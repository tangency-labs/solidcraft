//! Runner for unit `offset.offset_geometry_creation`. C++:
//! `solidcore_ffi::units::offset_offset_geometry_creation` (shim
//! `harness/cpp/shim/offset_offset_geometry_creation.cpp`). Rust:
//! `solidcore::offset::offset_geometry_creation`.
//!
//! **Two real B-reps.** Each case builds two `SmBrep`s on the C++ side (an upstream
//! `.smb` file, a wire edge, a face from a surface, a concrete cylinder/torus face, or the wire network of
//! `prog_test` `my_test_topology`; the second may be absent): the original B-rep of an
//! `SmOffsetExecutive` and the "offset" B-rep whose edge uses the executive maps the
//! original edge uses to (the pairs are given; `MapEdgeuses` is unit
//! `offset.offset_executive_p1`). The script first shapes the fixture (curves and
//! surfaces of a store, an edge's curve replaced by a store curve, an edge use's UV
//! trim curve, marked edge uses), then runs the unit's functions. The Rust side imports
//! the shim's exports of both B-reps (as the `topology.vertexuse` runner does) into two
//! [`Brep`]s behind one adapter ([`Two`], which implements the port's [`OffsetBrep`]
//! with the ported topology functions) and the ported executive
//! (`offset.offset_executive_p1`, behind the port's [`OffsetExecutive`]) holding the same
//! flags and pairs, and runs the same script. Before every op both sides seed the
//! executive's extended-surface cache with every face surface of both B-reps and every
//! store surface mapped to itself (the shim through `m_vSurfExtSurf`, the runner through
//! `insert_extended_surface`), so `GetCreateExtendedSurface` returns the base surface
//! unextended and `CreateExtendedSurface` (HANG-16, HANG-17) never runs. Both sides produce the same flat record:
//! per op its code, a status and its outputs (new geometry as curve blocks and surface
//! samples, B-rep edits as edge state blocks).
//!
//! **Selectors.** Generated handles are selectors taken modulo the entity count of
//! their kind (both sides resolve them the same way from the exports); store-curve
//! selectors are taken modulo the store curves of the dimension the op needs (null
//! when there is none). A `from` curve is computed at resolution from the imported
//! B-rep (the B-spline part of an edge's curve or an edge use's UV trim curve, scaled
//! and translated) and passed to both sides explicitly.
//!
//! **Guards.** The shim refuses (`-2`) what upstream dereferences without checking;
//! the topology-walking functions need every edge use of both B-reps to be a loop edge
//! use with edge, curve, vertex, mate, neighbours and a face with surface (so wire,
//! single-face and network B-reps only exercise the refusal), except `VertexOffset`,
//! which walks no loops and keeps only its per-edge checks (a curve on each edge; for a
//! single edge, a face with surface and a B-spline curve). `ReplaceEdgeCurves` with
//! a 3D curve and no UV curve is refused only when the curves run opposite (the shape
//! test is replicated). The runner mirrors every guard on the imported B-reps. The run
//! header takes any finite offset distance (zero included) and any finite tolerance that
//! is not negative: upstream neither crashes nor hangs at zero (every generated script
//! ran at each zero; the `*_zero_distance` and `*_zero_approx` cases keep the bases whose
//! compared output changes at zero), so only a negative tolerance is refused, and the
//! runner mirrors that refusal (`approx_negative`) as it does the B-rep guards.
//!
//! **Hang guards (`-6`, the script stops; the case defers).** The shim does not run two
//! ops the Rust side defers at anyway, as they can take minutes or not return (the
//! fuzz allows 20 s a side): a non-local `FaceuseOffset` of an exactly
//! `SmSurfOfRevolution` surface with self-intersection enabled (`upstream:HANG-26`),
//! or an unsupported concrete surface kind (`harness:analytic-geometry-export`), and an `EdgeuseOffset` with
//! `|offset distance| > 1e6` (deferred under `upstream:HANG-27`, the rail's
//! `SmCrvOnSurf::ApproximateCurve`). The generator's distances (0.001 to 100) never
//! reach the distance bound. HANG-26 remains guarded independently of exact imports.
//!
//! **Analytic faces.** The shared importer brings an `SmBSplineSurface` subclass in as its
//! B-spline part; revolution, extrusion, cone, cylinder, sphere and torus face surfaces
//! are then restored from the shim's binary `WriteToDB` records (`read_face_surfaces`)
//! through the existing polymorphic reader. Native type codes, complete record
//! consumption, face alignment and existing owner identities are checked. Restored
//! surfaces synchronize their cached tolerance from the actual owning face. No native
//! operation results are imported or replayed on the Rust side.
//!
//! **Deferred.** Operations dispatching on an unsupported, inexact face kind
//! (`harness:analytic-geometry-export`), the
//! trimmed surface cache of a face's own surface that the
//! seeded `GetCreateExtendedSurface` hands to the surface intersections of `MateEdges`
//! and `TrimExtendedEdge` (`SmTol::GetZoneTol3d`, `surface.surface_cache_p2`, and the face
//! handle, `topology.face_p2`), the drops inside `CreateUVTrimCurve`
//! (`intersect.surface_drop_lib_p3`, `intersect.surface_drop_curve`), the offset-surface
//! rails (`curve.crv_on_surf`) and the circular cross-section surface of `EdgeuseOffset`,
//! and the in-place notification and inset-on-shell-face mating (`SmBrep::Notify`,
//! `SmBrep::MateLaminaEdgeToSurface`, `topology.brep_p2`).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::{CurveKind, BSPLINE_CURVE_TYPE};
use solidcore::curve::curve::Curve as SmCurve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::context::MarkType;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::object::ObjectRef;
use solidcore::geom::solution_array::{ObjectKind, SolutionObject};
use solidcore::intersect::point_class::{ClassGeometry, ClassObject, ClassObjectKind};
use solidcore::kernel::Kernel;
use solidcore::math::{Point2, Point3, Vec3};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::crv_basic::Curve as NlCurve;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::offset::offset_executive::p1::{
    OffsetExecutive as OffsetExecutivePort, OffsetOperation,
};
use solidcore::offset::offset_geometry_creation::{
    LocalOperationGeometryCreation, OffsetBrep, OffsetExecutive, OffsetGeometryCreation, SsiData,
    UNDEFINED_UPSTREAM,
};
use solidcore::query::edge::ClosestPoint;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::p2::{
    DropCurveFail, SurfaceKind, TsectCurveType, CONE_TYPE, CYLINDER_TYPE, SPHERE_TYPE,
    SURF_OF_EXTRUSION_TYPE, SURF_OF_REVOLUTION_TYPE, TORUS_TYPE,
};
use solidcore::surface::surface::Surface;
use solidcore::surface::volume::Volume;
use solidcore::topology::database_io::{DatabaseReader, FileType, InputStream};
use solidcore::topology::edge as ed;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::edgeuse::p2 as eu2;
use solidcore::topology::edgeuse::p3 as eu3;
use solidcore::topology::face::p1 as fp1;
use solidcore::topology::face::p2 as fp2;
use solidcore::topology::proto_topology::ProtoGeometry;
use solidcore::topology::topology::Topology;
use solidcore::topology::vertex as vx;
use solidcore::topology::{
    Brep, EdgeId, EdgeuseId, FaceId, FaceuseId, ObjectHandle, VertexId, VertexuseId,
};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::offset_offset_geometry_creation::{self as ffi, Op};

use super::topology_vertexuse::import_with_types;
use crate::case::{CaseContext, CaseSource};
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The runner.
pub struct Runner;

const MIN_GENERATED: usize = 800;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg.into())
}

/// The shim's "upstream would dereference null" status.
const NULL_DEREF: f64 = -2.0;
/// The deferral label of an op whose upstream path dispatches a virtual surface method
/// on a face surface the importer could only bring in as its B-spline part (an
/// `SmBSplineSurface` subclass whose concrete record is not supported by this
/// adapter: no plan unit's port would unblock it, an exact import would.
const ANALYTIC_EXPORT: &str = "harness:analytic-geometry-export";
/// The native offset self-intersection of an exactly `SmSurfOfRevolution` surface
/// can take minutes (HANG-26); restoring its concrete kind does not remove this guard.
const HANG_26: &str = "upstream:HANG-26";
/// The deferral label of an `EdgeuseOffset` whose offset distance is beyond the
/// shim's HANG-27 bound (`plan/upstream-issues.md`): upstream's rail approximation does
/// not return in time there, so neither side runs the op.
const HANG_27: &str = "upstream:HANG-27";
/// The shim's `SURFACE_DB_VERSION`: the database version of concrete surface records.
const SURFACE_DB_VERSION: u64 = 33;
/// The shim's `MAX_EDGEUSE_OFFSET_DISTANCE` (HANG-27): a bound chosen for the fuzz
/// time budget (20 s a side; 10 s measured at 1e6), not the termination boundary
/// (~1e250).
const MAX_EDGEUSE_OFFSET_DISTANCE: f64 = 1.0e6;
/// Upstream's `SM_ERR_INVALID_INPUT` status (`SmMessages.h`), the status a library
/// `SolidError::InvalidInput` stands for.
const SM_ERR_INVALID_INPUT: f64 = 1007.0;

// ---- inputs ---------------------------------------------------------------------------------

/// A B-spline curve (`[x, y, z, w]` points; `z = NOZ` for 2D, `w = NOW` non-rational).
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
    /// A full, bounded native `SmCylinder` face, not a zero-angle `SmCone`.
    Cylinder {
        radius: f64,
        height: f64,
    },
    /// A full native `SmTorus` face, not a generic surface of revolution.
    Torus {
        major: f64,
        minor: f64,
    },
    Network,
}

/// A handle `[brep, kind, index]`; the index of a generated handle is a selector.
type Hin = [i64; 3];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    AddCurve {
        curve: CrvIn,
    },
    /// A store curve made from the B-spline part of an edge's curve (`src` an edge) or
    /// an edge use's UV trim curve (`src` an edge use), scaled about the origin and
    /// translated.
    AddCurveFrom {
        src: Hin,
        #[serde(default)]
        d: [f64; 3],
        #[serde(default = "one")]
        scale: f64,
    },
    AddSurface {
        surface: SrfIn,
    },
    SetEdgeCurve {
        e: Hin,
        c: Hin,
    },
    SetEuUv {
        eu: Hin,
        c: Hin,
        #[serde(default)]
        max_dist: f64,
    },
    Mark {
        eu: Hin,
    },
    Getters,
    FaceuseOffset {
        fu: Hin,
        #[serde(default)]
        skip: bool,
    },
    VertexOffset {
        v: Hin,
        edges: Vec<Hin>,
    },
    UpdateFaceDomain {
        f: Hin,
        s: Hin,
    },
    ReplaceEdgeCurves {
        eu: Hin,
        c3: Hin,
        uv: Hin,
        orig: Hin,
    },
    TrimExtendedEdge {
        base: Hin,
        other_base: Hin,
        off: Hin,
        other_off: Hin,
        c3: Hin,
        uv_base: Hin,
        uv_other: Hin,
    },
    ExtendTrimEdges {
        eu: Hin,
        other: Hin,
        off: Hin,
        other_off: Hin,
    },
    MateEdges {
        eu: Hin,
        other: Hin,
        off: Hin,
        other_off: Hin,
    },
    HaveTangentGaps {
        n: u64,
        off: Hin,
        other_off: Hin,
    },
    EdgeuseOffset {
        orig: Hin,
        other_orig: Hin,
        off: Hin,
        other_off: Hin,
    },
    LocalFaceuseOffset {
        fu: Hin,
        #[serde(default)]
        skip: bool,
    },
    EdgeState {
        e: Hin,
    },
    SetEdgeTol {
        e: Hin,
        tol: f64,
    },
}

fn one() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    brep_a: BrepIn,
    #[serde(default)]
    brep_b: Option<BrepIn>,
    dist: f64,
    approx: f64,
    #[serde(default)]
    analytics: bool,
    /// `(face, surface)` pairs of the local-operation creation.
    #[serde(default)]
    local: Vec<[Hin; 2]>,
    #[serde(default)]
    inset: bool,
    #[serde(default)]
    extend: bool,
    #[serde(default)]
    shell: Vec<Hin>,
    /// `(original edge use, offset edge use)` pairs of the executive.
    #[serde(default)]
    pairs: Vec<[Hin; 2]>,
    ops: Vec<OpIn>,
}

// ---- encoding ---------------------------------------------------------------------------

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

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
            let ku =
                b.u.len()
                    .checked_sub(1)
                    .ok_or_else(|| harness("surface without u knots"))?;
            let kv =
                b.v.len()
                    .checked_sub(1)
                    .ok_or_else(|| harness("surface without v knots"))?;
            v.extend([
                0.0,
                flag(b.oob),
                f64::from(b.form),
                n as f64,
                m as f64,
                b.p as f64,
                b.q as f64,
                ku as f64,
                kv as f64,
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

/// The `.smb` path of a B-rep input ("" when it is not a file).
fn path_of(b: Option<&BrepIn>) -> Result<String, HarnessError> {
    match b {
        Some(BrepIn::File { file, .. }) => {
            let p = crate::fixtures::upstream_test_file(&crate::case::repo_root(), file)?;
            Ok(p.to_string_lossy().into_owned())
        }
        _ => Ok(String::new()),
    }
}

fn brep_block(v: &mut Vec<f64>, b: &BrepIn) -> Result<(), HarnessError> {
    match b {
        BrepIn::File { rebuild, .. } => {
            v.push(0.0);
            v.push(flag(*rebuild));
        }
        BrepIn::Wire { curve, t0, t1 } => {
            v.push(1.0);
            push_curve(v, curve)?;
            v.extend([*t0, *t1]);
        }
        BrepIn::Face { surface, dom } => {
            v.push(2.0);
            push_surface(v, surface)?;
            v.extend(dom);
        }
        BrepIn::Cylinder { radius, height } => {
            v.extend([6.0, 0.0, *radius, *height]);
            return Ok(());
        }
        BrepIn::Torus { major, minor } => {
            v.extend([6.0, 1.0, *major, *minor]);
            return Ok(());
        }
        BrepIn::Network => v.push(3.0),
    }
    v.push(0.0); // validate_only
    v.push(0.0); // init_gaps
    Ok(())
}

/// The two B-rep blocks of a case.
fn blocks(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut v = Vec::new();
    brep_block(&mut v, &i.brep_a)?;
    match &i.brep_b {
        Some(b) => {
            v.push(1.0);
            brep_block(&mut v, b)?;
        }
        None => v.push(0.0),
    }
    Ok(v)
}

/// The C++ exports of the case's B-reps.
fn export(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let pa = path_of(Some(&i.brep_a))?;
    let pb = path_of(i.brep_b.as_ref())?;
    let block = blocks(i)?;
    crate::export_memo::export(
        "offset_offset_geometry_creation",
        &[&pa, &pb],
        &block,
        || ffi::eval(Op::Export, [&pa, &pb], &block),
    )
    .map_err(|e| harness(format!("c++ export: {e}")))
}

/// A build failure as an operation error.
fn build_failure(export: &[f64]) -> Output {
    Output::err(format!(
        "brep_build_{}_status_{}",
        if export.first() == Some(&-3.0) {
            "b"
        } else {
            "a"
        },
        export.get(1).copied().unwrap_or(0.0)
    ))
}

// ---- handles ------------------------------------------------------------------------------

/// Handle kinds (the shim's `HKind`, plus the Rust-only kinds).
const K_VERTEX: u8 = 0;
const K_EDGE: u8 = 1;
const K_FACE: u8 = 2;
const K_EDGE_CURVE: u8 = 3;
const K_FACE_SURFACE: u8 = 4;
const K_CURVE: u8 = 5;
const K_BREP: u8 = 6;
const K_SURFACE: u8 = 7;
const K_NULL: i64 = 8;
const K_EDGEUSE: u8 = 9;
const K_FACEUSE: u8 = 10;
const K_EU_UV: u8 = 11;
/// A curve the port made (Rust only).
const K_EXTRA: u8 = 12;
/// A surface the port made (Rust only).
const K_EXTRA_S: u8 = 13;
/// Internal cache-only topology handles; never encoded as native script inputs.
const K_VERTEXUSE: u8 = 14;
const K_LOOPUSE: u8 = 15;
const K_LOOP: u8 = 16;
const K_SHELL: u8 = 17;
const K_REGION: u8 = 18;

#[derive(Debug, Clone, Copy)]
struct H {
    b: i8,
    kind: u8,
    i: u32,
    ivl: [f64; 2],
}

impl PartialEq for H {
    fn eq(&self, o: &Self) -> bool {
        (self.b, self.kind, self.i) == (o.b, o.kind, o.i)
    }
}

impl Eq for H {}

impl PartialOrd for H {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

impl Ord for H {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        (self.b, self.kind, self.i).cmp(&(o.b, o.kind, o.i))
    }
}

const fn hd(b: i8, kind: u8, i: u32) -> H {
    H {
        b,
        kind,
        i,
        ivl: [f64::NAN, f64::NAN],
    }
}

/// A curve handle carrying the curve's natural interval.
fn curve_h(b: i8, kind: u8, i: u32, c: &SmCurve) -> H {
    let ivl = c.natural_interval();
    H {
        b,
        kind,
        i,
        ivl: [ivl.min(), ivl.max()],
    }
}

impl ClassObject for H {
    fn class_kind(&self) -> ClassObjectKind {
        match self.kind {
            K_VERTEX => ClassObjectKind::Vertex,
            K_EDGE => ClassObjectKind::Edge,
            K_FACE => ClassObjectKind::Face,
            K_EDGE_CURVE | K_CURVE | K_EXTRA | K_EU_UV => ClassObjectKind::Curve,
            K_FACE_SURFACE | K_SURFACE | K_EXTRA_S => ClassObjectKind::Surface,
            K_EDGEUSE => ClassObjectKind::Edgeuse,
            K_FACEUSE => ClassObjectKind::Faceuse,
            _ => ClassObjectKind::Other,
        }
    }
}

impl SolutionObject for H {
    fn kind(&self) -> ObjectKind {
        match self.kind {
            K_VERTEX => ObjectKind::Vertex,
            K_EDGE => ObjectKind::Edge,
            K_FACE => ObjectKind::Face,
            K_EDGE_CURVE | K_CURVE | K_EXTRA | K_EU_UV => ObjectKind::Curve,
            K_FACE_SURFACE | K_SURFACE | K_EXTRA_S => ObjectKind::Surface,
            _ => ObjectKind::Other,
        }
    }
    fn natural_interval(&self) -> Option<Extent1d> {
        match self.kind {
            K_EDGE_CURVE | K_CURVE | K_EXTRA | K_EU_UV if self.ivl[0].is_finite() => {
                Some(Extent1d::from_min_max(self.ivl[0], self.ivl[1]))
            }
            _ => None,
        }
    }
}

fn h_to_triple(h: Option<H>) -> Hin {
    match h {
        None => [-1, K_NULL, -1],
        Some(h) => [i64::from(h.b), i64::from(h.kind), i64::from(h.i)],
    }
}

fn triple_to_h(t: Hin) -> Option<H> {
    if t[1] == K_NULL {
        return None;
    }
    Some(hd(t[0] as i8, t[1] as u8, t[2] as u32))
}

// ---- the two B-reps behind the handles --------------------------------------------------

/// The topology records, identities and attribute store the notifications run with
/// (the `topology.vertex` / `topology.edge` runners' environment).
struct NotifyEnv {
    records: BTreeMap<ObjectKey, Topology>,
    attributes: AttributeStore,
    this: ObjectRef,
}

impl NotifyEnv {
    fn new(b: Option<&Brep>) -> Self {
        let mut records = BTreeMap::new();
        records.insert(ObjectKey(1), Topology::new());
        if let Some(b) = b {
            for id in b.vertexuses().ids() {
                records.insert(ObjectKey(100 + u64::from(id.index())), Topology::new());
            }
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

/// The identity a topology entity is notified under (the `topology.edge` runner's).
fn key_of(e: EntityRef) -> ObjectRef {
    let (type_code, kind, i) = match e {
        EntityRef::Brep(x) => (16011, 0u64, x.index()),
        EntityRef::Region(x) => (16012, 1, x.index()),
        EntityRef::Shell(x) => (16013, 2, x.index()),
        EntityRef::Faceuse(x) => (16022, 3, x.index()),
        EntityRef::Face(x) => (16002, 4, x.index()),
        EntityRef::Loopuse(x) => (16004, 5, x.index()),
        EntityRef::Loop(x) => (16003, 6, x.index()),
        EntityRef::Edgeuse(x) => (16017, 7, x.index()),
        EntityRef::Edge(x) => (ed::TYPE, 8, x.index()),
        EntityRef::Vertexuse(x) => (solidcore::topology::vertexuse::TYPE, 9, x.index()),
        EntityRef::Vertex(x) => (vx::TYPE, 10, x.index()),
    };
    ObjectRef {
        type_code,
        key: ObjectKey(1000 + kind * 100_000 + u64::from(i)),
    }
}

/// The two B-reps, the script's curve and surface stores (kinds 5 and 7), the port's
/// curves and surfaces (kinds 12 and 13) and the marked edge uses.
struct Two {
    breps: [Option<Brep>; 2],
    /// Per B-rep, per face (traversal order): whether the face's surface was imported
    /// exactly. Supported concrete subclasses are restored from their native
    /// records; unknown subclasses remain inexact B-spline stand-ins and cannot
    /// certify operations that dispatch on their kinds.
    exact: [Vec<bool>; 2],
    curves: Vec<SmCurve>,
    surfaces: Vec<Option<Surface>>,
    extra: Vec<SmCurve>,
    extra_s: Vec<Surface>,
    marked: BTreeSet<H>,
    env: [NotifyEnv; 2],
}

impl std::fmt::Debug for Two {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OffsetFaceQueries")
            .field("arenas", &self.breps.iter().filter(|b| b.is_some()).count())
            .finish_non_exhaustive()
    }
}

fn cache_topology_handle(b: i8, entity: EntityRef) -> H {
    let (kind, index) = match entity {
        EntityRef::Face(id) => (K_FACE, id.index()),
        EntityRef::Edge(id) => (K_EDGE, id.index()),
        EntityRef::Vertex(id) => (K_VERTEX, id.index()),
        EntityRef::Faceuse(id) => (K_FACEUSE, id.index()),
        EntityRef::Edgeuse(id) => (K_EDGEUSE, id.index()),
        EntityRef::Vertexuse(id) => (K_VERTEXUSE, id.index()),
        EntityRef::Loopuse(id) => (K_LOOPUSE, id.index()),
        EntityRef::Loop(id) => (K_LOOP, id.index()),
        EntityRef::Shell(id) => (K_SHELL, id.index()),
        EntityRef::Region(id) => (K_REGION, id.index()),
        EntityRef::Brep(_) => (K_BREP, 0),
    };
    hd(b, kind, index)
}

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input("difftest::offset_offset_geometry_creation::Two", reason)
}

const fn vid(h: H) -> VertexId {
    VertexId::from_index(h.i)
}
const fn eid(h: H) -> EdgeId {
    EdgeId::from_index(h.i)
}
const fn fid(h: H) -> FaceId {
    FaceId::from_index(h.i)
}
const fn euid(h: H) -> EdgeuseId {
    EdgeuseId::from_index(h.i)
}
const fn fuid(h: H) -> FaceuseId {
    FaceuseId::from_index(h.i)
}

impl Two {
    fn new(a: Brep, b: Option<Brep>, exact: [Vec<bool>; 2]) -> Self {
        let env = [NotifyEnv::new(Some(&a)), NotifyEnv::new(b.as_ref())];
        Self {
            breps: [Some(a), b],
            exact,
            curves: Vec::new(),
            surfaces: Vec::new(),
            extra: Vec::new(),
            extra_s: Vec::new(),
            marked: BTreeSet::new(),
            env,
        }
    }

    fn brep(&self, h: H) -> Option<&Brep> {
        self.breps.get(usize::try_from(h.b).ok()?)?.as_ref()
    }

    /// Whether `face`'s surface is the upstream kind on this side (see `exact`);
    /// `None` for a face outside the import table (no op creates faces, so a face
    /// handle always names an imported face; the caller refuses the rest).
    fn face_exact(&self, face: H) -> Option<bool> {
        usize::try_from(face.b)
            .ok()
            .and_then(|b| self.exact.get(b))
            .and_then(|v| v.get(usize::try_from(face.i).ok()?))
            .copied()
    }

    /// Whether `ExtendTrimEdges` of `eu` onto `off` lifts through `sm_ExtractIsoCurve` on a
    /// face imported as its B-spline part: the lift's UV curve (the edge use's or its
    /// radial's UV trim curve) is an iso line and `off`'s face is not exact. The C++ iso
    /// curve then keeps the subclass's kind (an `SmCircle` on a sphere, an `SmLine` on a
    /// surface of extrusion), which becomes the offset edge's curve, while the Rust
    /// lifts a B-spline.
    fn lifts_iso_on_inexact_face(&self, eu: H, off: H) -> solidcore::Result<bool> {
        match self.edgeuse_face(off)? {
            Some(face) if self.face_exact(face) != Some(true) => {}
            _ => return Ok(false),
        }
        let mut uses = vec![eu];
        if let Some(rad) = self.edgeuse_radial(eu)? {
            uses.push(rad);
        }
        for u in uses {
            let curve = self.edgeuse_uv_trim_curve(u)?;
            if curve
                .and_then(|h| ClassGeometry::curve(self, h))
                .is_some_and(is_uv_iso_segment)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Refuses (as [`ANALYTIC_EXPORT`]) an edit of a B-rep with a face whose surface was
    /// imported as its B-spline part: `MateLaminaEdgeToSurface` and `ReplaceSurface`
    /// branch on the surface kinds, which the Rust side would not see.
    fn require_exact_faces(&self, h: H) -> solidcore::Result<()> {
        let all = usize::try_from(h.b)
            .ok()
            .and_then(|b| self.exact.get(b))
            .ok_or_else(|| invalid("handle names an absent B-rep"))?;
        if all.iter().all(|&e| e) {
            Ok(())
        } else {
            Err(SolidError::Unported {
                upstream: "SmBrep::MateLaminaEdgeToSurface / ReplaceSurface (face surface imported as its B-spline part)",
                unit: ANALYTIC_EXPORT,
            })
        }
    }

    fn brep_mut(&mut self, h: H) -> Option<&mut Brep> {
        self.breps.get_mut(usize::try_from(h.b).ok()?)?.as_mut()
    }

    fn brep_r(&self, h: H) -> solidcore::Result<&Brep> {
        self.brep(h)
            .ok_or_else(|| invalid("handle names an absent B-rep"))
    }

    fn brep_m(&mut self, h: H) -> solidcore::Result<&mut Brep> {
        self.brep_mut(h)
            .ok_or_else(|| invalid("handle names an absent B-rep"))
    }

    fn eu_h(b: i8, eu: Option<EdgeuseId>) -> Option<H> {
        eu.map(|eu| hd(b, K_EDGEUSE, eu.index()))
    }

    /// The edge's curve handle.
    fn edge_curve_h(&self, e: H) -> solidcore::Result<Option<H>> {
        let b = self.brep_r(e)?;
        Ok(b.edge(eid(e))?
            .curve
            .as_ref()
            .map(|c| curve_h(e.b, K_EDGE_CURVE, e.i, c)))
    }

    /// The UV trim curve handle of an edge use (stored on it or its mate).
    fn eu_uv_h(&self, eu: H) -> solidcore::Result<Option<H>> {
        let b = self.brep_r(eu)?;
        let (c, owner) = eu1::uv_trim_curve(b, euid(eu))?;
        Ok(match (c, owner) {
            (Some(c), Some(owner)) => Some(curve_h(eu.b, K_EU_UV, owner.index(), c)),
            _ => None,
        })
    }

    /// Whether every edge use of the B-rep is a loop edge use with edge (with curve),
    /// vertex, mate, neighbours and a face with surface, and every edge has a vertex and
    /// a curve (the shim's `brep_ok`).
    fn brep_ok(&self, slot: usize) -> bool {
        let Some(b) = self.breps.get(slot).and_then(Option::as_ref) else {
            return false;
        };
        for eu in b.edgeuses().ids() {
            let ok = (|| -> solidcore::Result<bool> {
                let Some(e) = eu1::edge(b, eu)? else {
                    return Ok(false);
                };
                if b.edge(e)?.curve.is_none() {
                    return Ok(false);
                }
                let Some(vu) = eu1::vertexuse(b, eu)? else {
                    return Ok(false);
                };
                if b.vertexuse(vu)?.vertex.is_none() {
                    return Ok(false);
                }
                if eu1::mate(b, eu)?.is_none() || !eu1::is_loop_edgeuse(b, eu)? {
                    return Ok(false);
                }
                if eu1::cw_edgeuse(b, eu)?.is_none() || eu1::ccw_edgeuse(b, eu)?.is_none() {
                    return Ok(false);
                }
                if eu1::faceuse(b, eu)?.is_none() {
                    return Ok(false);
                }
                let Some(f) = eu1::face(b, eu)? else {
                    return Ok(false);
                };
                Ok(b.face(f)?.surface.is_some())
            })();
            if !ok.unwrap_or(false) {
                return false;
            }
        }
        for e in b.edges().ids() {
            let ok = ed::vertex(b, e).ok().flatten().is_some()
                && b.edge(e).is_ok_and(|edge| edge.curve.is_some());
            if !ok {
                return false;
            }
        }
        true
    }

    fn all_ok(&self) -> bool {
        self.brep_ok(0) && (self.breps[1].is_none() || self.brep_ok(1))
    }

    /// `SmEdge::SetCurve(copy, TRUE)` + `SetOwner(edge)`: the curve behind `c` becomes
    /// the edge's curve (the old curve and the UV trim curves are dropped).
    fn set_edge_curve_from(&mut self, e: H, curve: SmCurve) -> solidcore::Result<()> {
        let slot = usize::try_from(e.b).map_err(|_| invalid("bad B-rep"))?;
        let (breps, env) = (&mut self.breps, &mut self.env);
        let b = breps
            .get_mut(slot)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("handle names an absent B-rep"))?;
        let env = env.get_mut(slot).ok_or_else(|| invalid("bad B-rep"))?;
        let mut nenv = ed::NotifyEnv {
            records: &mut env.records,
            attributes: &mut env.attributes,
            context: None,
            key_of: &key_of,
        };
        let opts = ed::SetCurveOptions {
            delete_old_curve: true,
            dbg_warn_leaks: true,
            call_notify: true,
        };
        ed::set_curve(b, eid(e), Some(curve), opts, &mut nenv, [None, None])?;
        if let Some(c) = b.edge_mut(eid(e))?.curve.as_mut() {
            c.set_owner(Some(key_of(EntityRef::Edge(eid(e)))), None);
        }
        Ok(())
    }

    fn set_eu_uv_from(
        &mut self,
        eu: H,
        curve: Option<SmCurve>,
        max_dist: f64,
    ) -> solidcore::Result<()> {
        let b = self.brep_m(eu)?;
        let arg = match curve {
            Some(c) => eu1::UvTrimCurveArg::New(Box::new(c)),
            None => eu1::UvTrimCurveArg::Null,
        };
        eu1::set_uv_trim_curve(b, euid(eu), arg, max_dist, true, None)?;
        Ok(())
    }

    /// `edge_state`: curve, interval, tolerance, the vertex points and the edge uses' UV
    /// trim curves.
    fn put_edge_state(&self, o: &mut Vec<f64>, e: H) -> Result<(), HarnessError> {
        let b = self.brep_r(e).map_err(|e| harness(format!("{e}")))?;
        let edge = b.edge(eid(e)).map_err(|e| harness(format!("{e}")))?;
        put_curve(o, edge.curve.as_ref())?;
        o.extend([edge.interval.min(), edge.interval.max(), edge.tolerance]);
        match ed::vertex(b, eid(e)).map_err(|e| harness(format!("{e}")))? {
            Some(v) => {
                o.push(1.0);
                put_vec(o, vx::point(b, v).map_err(|e| harness(format!("{e}")))?);
                match ed::other_vertex(b, eid(e), Some(v)).map_err(|e| harness(format!("{e}")))? {
                    Some(w) => {
                        o.push(1.0);
                        put_vec(o, vx::point(b, w).map_err(|e| harness(format!("{e}")))?);
                    }
                    None => o.push(0.0),
                }
            }
            None => o.push(0.0),
        }
        let eus = ed::edgeuses(b, eid(e), None, None).map_err(|e| harness(format!("{e}")))?;
        o.push(eus.len() as f64);
        for eu in eus {
            let (c, _) = eu1::uv_trim_curve(b, eu).map_err(|e| harness(format!("{e}")))?;
            put_curve(o, c)?;
        }
        Ok(())
    }
}

impl ClassGeometry<H> for Two {
    fn curve(&self, o: H) -> Option<&SmCurve> {
        match o.kind {
            K_EDGE_CURVE => self.brep(o)?.edge(eid(o)).ok()?.curve.as_ref(),
            K_EU_UV => self.brep(o)?.edgeuse(euid(o)).ok()?.uv_trim_curve.as_ref(),
            K_CURVE => self.curves.get(o.i as usize),
            K_EXTRA => self.extra.get(o.i as usize),
            _ => None,
        }
    }
    fn surface(&self, o: H) -> Option<&Surface> {
        match o.kind {
            K_FACE_SURFACE => self.brep(o)?.face(fid(o)).ok()?.surface.as_ref(),
            K_SURFACE => self.surfaces.get(o.i as usize)?.as_ref(),
            K_EXTRA_S => self.extra_s.get(o.i as usize),
            _ => None,
        }
    }
    fn volume(&self, _o: H) -> Option<&Volume> {
        None
    }
    fn point(&self, _o: H) -> Option<Point3> {
        None
    }
}

impl ProtoGeometry<H> for Two {
    fn curve_mut(&mut self, o: H) -> Option<&mut SmCurve> {
        match o.kind {
            K_EDGE_CURVE => self.brep_mut(o)?.edge_mut(eid(o)).ok()?.curve.as_mut(),
            K_EU_UV => self
                .brep_mut(o)?
                .edgeuse_mut(euid(o))
                .ok()?
                .uv_trim_curve
                .as_mut(),
            K_CURVE => self.curves.get_mut(o.i as usize),
            K_EXTRA => self.extra.get_mut(o.i as usize),
            _ => None,
        }
    }
}

impl solidcore::surface::surface_cache::face_context::FaceQueries<H> for Two {
    fn face_zone_tol_3d(&self, face: H) -> solidcore::Result<f64> {
        if face.kind != K_FACE {
            return Err(invalid("cache classifier expected a face handle"));
        }
        fp2::tolerance(self.brep_r(face)?, fid(face))
    }

    fn face_point_classify(
        &self,
        k: &Kernel,
        face: H,
        uv: Point2,
        src_tol: f64,
    ) -> solidcore::Result<(i32, Option<H>)> {
        use solidcore::surface::surface_cache::face_context::PreparedFaceQueries;
        if face.kind != K_FACE {
            return Err(invalid("cache classifier expected a face handle"));
        }
        let map = |entity| Ok(cache_topology_handle(face.b, entity));
        PreparedFaceQueries::new(self.brep_r(face)?, fid(face), &map)?
            .face_point_classify(k, face, uv, src_tol)
    }

    fn face_point_classify_fast_drop(
        &self,
        k: &Kernel,
        face: H,
        uv: Point2,
        src_tol: f64,
    ) -> solidcore::Result<(i32, Option<H>)> {
        use solidcore::surface::surface_cache::face_context::PreparedFaceQueries;
        if face.kind != K_FACE {
            return Err(invalid("cache classifier expected a face handle"));
        }
        let map = |entity| Ok(cache_topology_handle(face.b, entity));
        PreparedFaceQueries::new(self.brep_r(face)?, fid(face), &map)?
            .face_point_classify_fast_drop(k, face, uv, src_tol)
    }
}

impl OffsetBrep<H> for Two {
    fn prepare_surface_solve_cache(
        &mut self,
        k: &Kernel,
        surface: &Surface,
        handle: Option<H>,
    ) -> solidcore::Result<solidcore::surface::surface_cache::SurfaceCache<H>> {
        use solidcore::query::face::p2::BrepCacheFace;
        use solidcore::surface::surface::p1::{
            get_surface_cache, make_surface_cache, make_trim_srf_cache_with_face,
        };
        use solidcore::surface::surface_cache::face_context::MappedCacheFace;
        use solidcore::surface::trim_srf_cache::CacheFace;
        let Some(owner) = surface.face()? else {
            return get_surface_cache(k, surface, handle);
        };
        // The surface handle selects its arena. ObjectRef keys alone deliberately
        // cannot distinguish equal face indices of these independent B-reps.
        let Some(h) = handle.filter(|h| h.kind == K_FACE_SURFACE && (h.b == 0 || h.b == 1)) else {
            return get_surface_cache(k, surface, handle);
        };
        let b = self.brep_m(h)?;
        // The typed surface handle already names its face; the imported owner
        // key belongs to the importer's object space, not the notification store's
        // `key_of` convention. Validate the actual record instead of conflating
        // those independent key mappings.
        let face = fid(h);
        let actual_owner = fp2::surface(b, face)?
            .ok_or_else(|| invalid("cache face has no surface"))?
            .face()?;
        if actual_owner != Some(owner) {
            return Err(invalid("surface owner is not the selected B-rep face"));
        }
        let map = |entity| Ok(cache_topology_handle(h.b, entity));
        let unmap = |handle: H| {
            if handle.b != h.b {
                return Err(invalid("cache handle belongs to a different B-rep"));
            }
            Ok(match handle.kind {
                K_FACE => EntityRef::Face(FaceId::from_index(handle.i)),
                K_EDGE => EntityRef::Edge(EdgeId::from_index(handle.i)),
                K_VERTEX => EntityRef::Vertex(VertexId::from_index(handle.i)),
                K_EDGEUSE => EntityRef::Edgeuse(EdgeuseId::from_index(handle.i)),
                K_VERTEXUSE => EntityRef::Vertexuse(VertexuseId::from_index(handle.i)),
                K_LOOPUSE => {
                    EntityRef::Loopuse(solidcore::topology::LoopuseId::from_index(handle.i))
                }
                _ => return Err(invalid("unsupported typed cache handle")),
            })
        };
        let mut context = MappedCacheFace::new(BrepCacheFace { b, f: face }, &map, &unmap)?;
        match make_trim_srf_cache_with_face(k, surface, handle, &mut context) {
            Ok(mut cache) => {
                cache.bind_face(
                    owner,
                    cache_topology_handle(h.b, EntityRef::Face(face)),
                    context.zone_tol_3d(k, context.face())?,
                )?;
                if cache.base().tree().is_none() {
                    cache.build_tree_with_face(k, &mut context)?;
                }
                Ok(cache.into_base())
            }
            Err(
                e @ (SolidError::Unported { .. }
                | SolidError::InvalidInput { .. }
                | SolidError::NotFound { .. }),
            ) => Err(e),
            Err(_) => make_surface_cache(k, surface, handle),
        }
    }

    fn prepared_face_queries(
        &self,
    ) -> Option<&dyn solidcore::surface::surface_cache::face_context::FaceQueries<H>> {
        Some(self)
    }

    fn insert_curve(&mut self, curve: SmCurve) -> Option<H> {
        let i = u32::try_from(self.extra.len()).ok()?;
        let hh = curve_h(2, K_EXTRA, i, &curve);
        self.extra.push(curve);
        Some(hh)
    }
    fn insert_surface(&mut self, surface: Surface) -> Option<H> {
        let i = u32::try_from(self.extra_s.len()).ok()?;
        self.extra_s.push(surface);
        Some(hd(2, K_EXTRA_S, i))
    }
    fn surface_mut(&mut self, o: H) -> Option<&mut Surface> {
        match o.kind {
            K_FACE_SURFACE => self.brep_mut(o)?.face_mut(fid(o)).ok()?.surface.as_mut(),
            K_SURFACE => self.surfaces.get_mut(o.i as usize)?.as_mut(),
            K_EXTRA_S => self.extra_s.get_mut(o.i as usize),
            _ => None,
        }
    }
    fn brep_tolerance(&self, topo: H) -> solidcore::Result<f64> {
        Ok(self.brep_r(topo)?.tolerance())
    }
    fn faceuse_face(&self, fu: H) -> solidcore::Result<Option<H>> {
        Ok(eu1::faceuse_face(self.brep_r(fu)?, fuid(fu))?.map(|f| hd(fu.b, K_FACE, f.index())))
    }
    fn faceuse_orientation(&self, fu: H) -> solidcore::Result<OrientType> {
        eu1::faceuse_orientation(self.brep_r(fu)?, fuid(fu))
    }
    fn face_surface(&self, face: H) -> solidcore::Result<Option<H>> {
        let b = self.brep_r(face)?;
        Ok(b.face(fid(face))?
            .surface
            .as_ref()
            .map(|_| hd(face.b, K_FACE_SURFACE, face.i)))
    }
    fn face_uv_domain(&self, face: H) -> solidcore::Result<Extent2d> {
        fp2::uv_domain(self.brep_r(face)?, fid(face))
    }
    fn set_face_uv_domain(&mut self, face: H, domain: Extent2d) -> solidcore::Result<()> {
        fp2::set_uv_domain(self.brep_m(face)?, fid(face), &domain)
    }
    fn face_uv_domain_from_uv_trim_curves(
        &mut self,
        k: &Kernel,
        face: H,
    ) -> solidcore::Result<Extent2d> {
        let b = self.brep_m(face)?;
        Ok(
            fp1::calculate_uv_domain_from_uv_trim_curves(k, b, fid(face), true, false, false)?
                .domain,
        )
    }
    fn edge_curve(&self, edge: H) -> solidcore::Result<Option<H>> {
        self.edge_curve_h(edge)
    }
    fn edge_interval(&self, edge: H) -> solidcore::Result<Extent1d> {
        ed::interval(self.brep_r(edge)?, eid(edge))
    }
    fn set_edge_interval(&mut self, edge: H, interval: Extent1d) -> solidcore::Result<()> {
        ed::set_interval(self.brep_m(edge)?, eid(edge), &interval)
    }
    fn edge_tolerance(&self, edge: H) -> solidcore::Result<f64> {
        ed::tolerance(self.brep_r(edge)?, eid(edge))
    }
    fn set_edge_tolerance(
        &mut self,
        _k: &Kernel,
        edge: H,
        tolerance: f64,
    ) -> solidcore::Result<()> {
        ed::set_tolerance(self.brep_m(edge)?, eid(edge), tolerance, true, true)
    }
    fn edge_set_curve(&mut self, _k: &Kernel, edge: H, curve: H) -> solidcore::Result<()> {
        let c = ClassGeometry::curve(self, curve)
            .ok_or_else(|| invalid("curve handle cannot be resolved"))?
            .clone();
        self.set_edge_curve_from(edge, c)
    }
    fn brep_mate_lamina_edge_to_surface(
        &mut self,
        k: &Kernel,
        edge: H,
        surface: H,
        surface_extension_factor: f64,
        uv_curve_extension_factor: f64,
    ) -> solidcore::Result<()> {
        if edge.kind != K_EDGE {
            return Err(invalid("mate of a non-edge handle"));
        }
        self.require_exact_faces(edge)?;
        // a const reference upstream: the mating surface is only read
        let mating = ClassGeometry::surface(self, surface)
            .ok_or_else(|| invalid("surface handle cannot be resolved"))?
            .clone();
        let b = self.brep_m(edge)?;
        let mut state = solidcore::topology::brep::p2::ArenaNotifyState::new(b);
        solidcore::topology::brep::p2::mate_lamina_edge_to_surface(
            k,
            b,
            &mut state.env(None),
            eid(edge),
            &mating,
            surface_extension_factor,
            uv_curve_extension_factor,
        )
    }
    fn brep_replace_surface(
        &mut self,
        k: &Kernel,
        face: H,
        surface: H,
        add_to_deleted_list: bool,
        create_uv_trim_curves: bool,
    ) -> solidcore::Result<()> {
        if face.kind != K_FACE {
            return Err(invalid("surface replacement of a non-face handle"));
        }
        self.require_exact_faces(face)?;
        // the face takes the surface; its store entry is left unreferenced
        let new_surface = ClassGeometry::surface(self, surface)
            .ok_or_else(|| invalid("surface handle cannot be resolved"))?
            .clone();
        let b = self.brep_m(face)?;
        let mut state = solidcore::topology::brep::p2::ArenaNotifyState::new(b);
        let old = if add_to_deleted_list {
            solidcore::topology::brep::p2::OldSurface::AddToDeletedList
        } else {
            solidcore::topology::brep::p2::OldSurface::Delete
        };
        let mut env = state.env(None);
        let face_ref = (env.key_of)(EntityRef::Face(fid(face)));
        solidcore::topology::brep::p2::replace_surface(
            k,
            b,
            &mut env,
            fid(face),
            new_surface,
            face_ref,
            old,
            create_uv_trim_curves,
        )
        .map(|_| ())
    }
    fn edge_faces(&self, edge: H) -> solidcore::Result<Vec<H>> {
        let b = self.brep_r(edge)?;
        Ok(ed::faces(b, eid(edge), None)?
            .into_iter()
            .map(|f| hd(edge.b, K_FACE, f.index()))
            .collect())
    }
    fn edge_edgeuses(&self, edge: H) -> solidcore::Result<Vec<H>> {
        let b = self.brep_r(edge)?;
        Ok(ed::edgeuses(b, eid(edge), None, None)?
            .into_iter()
            .map(|eu| hd(edge.b, K_EDGEUSE, eu.index()))
            .collect())
    }
    fn edge_is_lamina(&self, edge: H) -> solidcore::Result<bool> {
        ed::is_lamina(self.brep_r(edge)?, eid(edge))
    }
    fn edge_is_seam_of_face_surface(
        &mut self,
        k: &Kernel,
        edge: H,
        face: H,
    ) -> solidcore::Result<bool> {
        if face.b != edge.b || face.kind != K_FACE {
            return Err(invalid("face of another B-rep"));
        }
        ed::is_seam_of_surface(k, self.brep_m(edge)?, eid(edge), fid(face))
    }
    fn edge_vertex(&self, edge: H) -> solidcore::Result<Option<H>> {
        Ok(ed::vertex(self.brep_r(edge)?, eid(edge))?.map(|v| hd(edge.b, K_VERTEX, v.index())))
    }
    fn edge_other_vertex(&self, edge: H, vertex: H) -> solidcore::Result<Option<H>> {
        let b = self.brep_r(edge)?;
        let v = (vertex.b == edge.b && vertex.kind == K_VERTEX).then(|| vid(vertex));
        Ok(ed::other_vertex(b, eid(edge), v)?.map(|v| hd(edge.b, K_VERTEX, v.index())))
    }
    fn edge_closest_point(
        &self,
        k: &Kernel,
        edge: H,
        point: Point3,
    ) -> solidcore::Result<Option<ClosestPoint>> {
        solidcore::query::edge::closest_point(k, self.brep_r(edge)?, eid(edge), point)
    }
    fn edge_create_trimmed_nurbs_curve(
        &mut self,
        k: &Kernel,
        edge: H,
    ) -> solidcore::Result<Option<H>> {
        let c = ed::create_trimmed_nurbs_curve(k, self.brep_r(edge)?, eid(edge))?;
        Ok(match c {
            Some(c) => Some(
                self.insert_curve(c)
                    .ok_or_else(|| invalid("curve store is full"))?,
            ),
            None => None,
        })
    }
    fn vertex_point(&self, vertex: H) -> solidcore::Result<Point3> {
        vx::point(self.brep_r(vertex)?, vid(vertex))
    }
    fn set_vertex_point(&mut self, _k: &Kernel, vertex: H, point: Point3) -> solidcore::Result<()> {
        let slot = usize::try_from(vertex.b).map_err(|_| invalid("bad B-rep"))?;
        let (breps, env) = (&mut self.breps, &mut self.env);
        let b = breps
            .get_mut(slot)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("handle names an absent B-rep"))?;
        let env = env.get_mut(slot).ok_or_else(|| invalid("bad B-rep"))?;
        vx::set_point(
            b,
            vid(vertex),
            point,
            &mut env.records,
            &mut env.attributes,
            None,
            env.this,
            &vertexuse_ref,
        )
    }
    fn vertex_is_lamina(&self, vertex: H) -> solidcore::Result<bool> {
        vx::is_lamina_vertex(self.brep_r(vertex)?, vid(vertex))
    }
    fn vertex_are_edges_tangent(
        &self,
        vertex: H,
        e1: H,
        e2: H,
        ang_tol_deg: f64,
    ) -> solidcore::Result<bool> {
        let k = &Kernel::default();
        if e1.b != vertex.b || e2.b != vertex.b {
            return Err(invalid("edges of another B-rep"));
        }
        vx::are_edges_tangent(
            k,
            self.brep_r(vertex)?,
            vid(vertex),
            eid(e1),
            eid(e2),
            ang_tol_deg,
        )
    }
    fn edgeuse_edge(&self, eu: H) -> solidcore::Result<Option<H>> {
        Ok(eu1::edge(self.brep_r(eu)?, euid(eu))?.map(|e| hd(eu.b, K_EDGE, e.index())))
    }
    fn edgeuse_mate(&self, eu: H) -> solidcore::Result<Option<H>> {
        Ok(Self::eu_h(eu.b, eu1::mate(self.brep_r(eu)?, euid(eu))?))
    }
    fn edgeuse_radial(&self, eu: H) -> solidcore::Result<Option<H>> {
        Ok(Self::eu_h(eu.b, eu1::radial(self.brep_r(eu)?, euid(eu))?))
    }
    fn edgeuse_cw(&self, eu: H) -> solidcore::Result<Option<H>> {
        Ok(Self::eu_h(
            eu.b,
            eu1::cw_edgeuse(self.brep_r(eu)?, euid(eu))?,
        ))
    }
    fn edgeuse_ccw(&self, eu: H) -> solidcore::Result<Option<H>> {
        Ok(Self::eu_h(
            eu.b,
            eu1::ccw_edgeuse(self.brep_r(eu)?, euid(eu))?,
        ))
    }
    fn edgeuse_vertex(&self, eu: H) -> solidcore::Result<Option<H>> {
        let b = self.brep_r(eu)?;
        let Some(vu) = eu1::vertexuse(b, euid(eu))? else {
            return Ok(None);
        };
        Ok(b.vertexuse(vu)?
            .vertex
            .map(|v| hd(eu.b, K_VERTEX, v.index())))
    }
    fn edgeuse_face(&self, eu: H) -> solidcore::Result<Option<H>> {
        Ok(eu1::face(self.brep_r(eu)?, euid(eu))?.map(|f| hd(eu.b, K_FACE, f.index())))
    }
    fn edgeuse_faceuse(&self, eu: H) -> solidcore::Result<Option<H>> {
        Ok(eu1::faceuse(self.brep_r(eu)?, euid(eu))?.map(|f| hd(eu.b, K_FACEUSE, f.index())))
    }
    fn edgeuse_uv_trim_curve(&self, eu: H) -> solidcore::Result<Option<H>> {
        self.eu_uv_h(eu)
    }
    fn edgeuse_set_uv_trim_curve(
        &mut self,
        _k: &Kernel,
        eu: H,
        curve: Option<H>,
        max_dist: f64,
    ) -> solidcore::Result<()> {
        let c = match curve {
            Some(h) => Some(
                ClassGeometry::curve(self, h)
                    .ok_or_else(|| invalid("curve handle cannot be resolved"))?
                    .clone(),
            ),
            None => None,
        };
        self.set_eu_uv_from(eu, c, max_dist)
    }
    fn edgeuse_is_tangent_sector(
        &mut self,
        k: &Kernel,
        eu: H,
        tol_deg: f64,
    ) -> solidcore::Result<bool> {
        eu2::is_tangent_sector(k, self.brep_m(eu)?, euid(eu), tol_deg, false, true)
    }
    fn edgeuse_is_convex_radial_sector(
        &mut self,
        k: &Kernel,
        eu: H,
        num_samples: u64,
    ) -> solidcore::Result<bool> {
        eu2::is_convex_radial_sector(k, self.brep_m(eu)?, euid(eu), num_samples, false, 0.0)
    }
    fn edgeuse_is_marked(&self, _k: &Kernel, eu: H, _mark: MarkType) -> solidcore::Result<bool> {
        Ok(self.marked.contains(&eu))
    }
    fn edgeuse_normalized_evaluate(
        &mut self,
        k: &Kernel,
        eu: H,
        t: f64,
        parameter_space_eval: bool,
        want_first_deriv: bool,
        ok_to_make_uv_trim_curve: bool,
    ) -> solidcore::Result<Point3> {
        Ok(eu3::normalized_evaluate(
            k,
            self.brep_m(eu)?,
            euid(eu),
            t,
            parameter_space_eval,
            want_first_deriv,
            ok_to_make_uv_trim_curve,
        )?
        .point)
    }
    fn edgeuse_create_uv_trim_curve(
        &mut self,
        k: &Kernel,
        eu: H,
        want_max_deviation: bool,
        want_drop_curve_fail: bool,
    ) -> solidcore::Result<(f64, SmCurve)> {
        let mut fail = DropCurveFail::<ObjectHandle>::default();
        let made = eu3::create_uv_trim_curve(
            k,
            self.brep_m(eu)?,
            euid(eu),
            want_max_deviation,
            want_drop_curve_fail.then_some(&mut fail),
        )?;
        Ok((made.max_dist_to_surf, made.curve))
    }
}

/// The executive: the ported `SmOffsetExecutive` (`offset.offset_executive_p1`) holding the
/// flags, the shell faces, the original-to-offset edge-use relation (many-to-many, as
/// `m_vEUToEU`) and the extended-surface cache. Its typed ids each name one B-rep, while the
/// script's handles span both, so a handle is folded into an id ([`exec_index`]) and back.
struct Exec {
    inner: OffsetExecutivePort,
    /// Extension-cache keys of the surfaces the shim seeds `m_vSurfExtSurf` with: every face
    /// surface of both B-reps and every store surface as they stand at the start of each op,
    /// cumulative as the C++ map (see [`Exec::seed`]).
    seeded: BTreeSet<u64>,
    /// Store handles of the extensions the port made for unseeded bases, by key.
    made: BTreeMap<u64, H>,
}

/// A handle as an executive id: `i * 64 + kind * 4 + (b + 1)` (the FFI admits `b` in
/// `-1..=2` and kinds below 12).
fn exec_index(h: H) -> Result<u32, HarnessError> {
    let b = u32::try_from(i16::from(h.b) + 1).ok().filter(|b| *b < 4);
    let kind = u32::from(h.kind);
    b.filter(|_| kind < 16)
        .and_then(|b| h.i.checked_mul(64)?.checked_add(kind * 4 + b))
        .ok_or_else(|| harness("executive handle outside the folded id range"))
}

/// The handle of an executive id ([`exec_index`]).
fn exec_handle(x: u32) -> H {
    hd((x & 3) as i8 - 1, ((x >> 2) & 15) as u8, x >> 6)
}

/// The extension-cache key of a surface handle (upstream keys the cache by pointer; each
/// imported face owns its surface, so a handle names one surface).
fn surface_key(h: H) -> u64 {
    (u64::from(h.i) << 16) | (u64::from(h.kind) << 8) | u64::from(h.b as u8)
}

impl Exec {
    /// Records the shim's per-op seeding (`seed_extended`): every face surface of both
    /// B-reps and every store surface. The surfaces themselves go into the port's cache at
    /// lookup ([`OffsetExecutive::get_create_extended_surface`]), as they stand then: the
    /// C++ map holds the live surface, not a copy.
    fn seed(&mut self, two: &Two) {
        for (b, brep) in two.breps.iter().enumerate() {
            let Some(brep) = brep else { continue };
            for (id, face) in brep.faces().iter() {
                if face.surface.is_some() {
                    self.seeded
                        .insert(surface_key(hd(b as i8, K_FACE_SURFACE, id.index())));
                }
            }
        }
        for (q, s) in two.surfaces.iter().enumerate() {
            if s.is_some() {
                self.seeded.insert(surface_key(hd(2, K_SURFACE, q as u32)));
            }
        }
    }
}

impl OffsetExecutive<H> for Exec {
    fn inset(&self) -> bool {
        self.inner.inset()
    }
    fn extend_convex_edges(&self) -> bool {
        self.inner.extend_convex_edges()
    }
    fn shell_faces(&self) -> Vec<H> {
        self.inner
            .shell_faces()
            .iter()
            .map(|f| exec_handle(f.index()))
            .collect()
    }
    fn offset_edgeuse(&self, original: H) -> Option<H> {
        let original = EdgeuseId::from_index(exec_index(original).ok()?);
        self.inner
            .offset_edgeuse(original)
            .map(|eu| exec_handle(eu.index()))
    }
    fn get_create_extended_surface<B: OffsetBrep<H>>(
        &mut self,
        k: &Kernel,
        brep: &mut B,
        base: H,
    ) -> solidcore::Result<(H, Extent2d)> {
        let surface = ClassGeometry::surface(brep, base)
            .ok_or_else(|| invalid("GetCreateExtendedSurface of a null surface"))?
            .clone();
        let key = surface_key(base);
        let seeded = self.seeded.contains(&key);
        if seeded {
            self.inner.insert_extended_surface(key, surface.clone());
        }
        let ext = self.inner.get_create_extended_surface(k, key, &surface)?;
        let domain = ext.natural_uv_domain();
        if seeded {
            // the seeded entry is the base itself, as `m_vSurfExtSurf.Insert(s, s)`
            return Ok((base, domain));
        }
        if let Some(&h) = self.made.get(&key) {
            return Ok((h, domain));
        }
        let ext = ext.clone();
        let h = brep
            .insert_surface(ext)
            .ok_or_else(|| invalid("surface store full"))?;
        self.made.insert(key, h);
        Ok((h, domain))
    }
}

// ---- geometry in and out ------------------------------------------------------------------

fn rust_curve(c: &CrvIn) -> Result<SmCurve, HarnessError> {
    let pw = c
        .cpts
        .iter()
        .map(|p| CPoint::new(p[0], p[1], p[2], p[3]))
        .collect();
    let cur = NlCurve::new(pw, c.p, c.u.clone()).map_err(|e| harness(format!("curve: {e}")))?;
    Ok(SmCurve::from(BSplineCurve::from_nurb(cur, c.dim, false)))
}

fn put_vec(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

/// The fixture header's `put_curve` block.
fn put_curve(out: &mut Vec<f64>, c: Option<&SmCurve>) -> Result<(), HarnessError> {
    let Some(c) = c else {
        out.push(-1.0);
        return Ok(());
    };
    // every SmBSplineCurve kind: its type and NURB (an SmLine adds its line state); an
    // iso curve on an exactly imported SmSurfOfRevolution is an SmCircle / SmEllipse
    let base = match c.kind() {
        CurveKind::BSpline(b) => b,
        CurveKind::Line(l) => l.base(),
        CurveKind::Ellipse(e) => e.base(),
        CurveKind::Hyperbola(h) => h.base(),
        CurveKind::Parabola(p) => p.base(),
        _ => {
            if c.is_kind_of(BSPLINE_CURVE_TYPE) {
                return Err(harness(
                    "put_curve: a B-spline kind this runner does not export",
                ));
            }
            out.push(-2.0);
            return Ok(());
        }
    };
    out.push(c.type_code() as f64);
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
        out.extend([
            p.x,
            p.y,
            p.z,
            v.x,
            v.y,
            v.z,
            d.min(),
            d.max(),
            l.line_scale(),
        ]);
        out.push(flag(l.inside_out()));
    }
    Ok(())
}

/// The upstream status code of an error the surface samples report.
fn status_code(e: &SolidError) -> f64 {
    match e {
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// The shim's `put_srf`: type, natural domain, a 3x3 point sample and, for a plain
/// B-spline surface or a surface of revolution, the NURB (`has_nurb [n m p q r s net
/// u_knots v_knots]`).
fn put_srf(k: &Kernel, o: &mut Vec<f64>, s: Option<&Surface>) {
    let Some(s) = s else {
        o.push(-1.0);
        return;
    };
    o.push(s.type_code() as f64);
    let d = s.natural_uv_domain();
    o.extend([d.u_min(), d.u_max(), d.v_min(), d.v_max()]);
    for i in 0..=2 {
        for j in 0..=2 {
            let uv = Point2::new(
                d.u_min() + (d.u_max() - d.u_min()) * (f64::from(i) / 2.0),
                d.v_min() + (d.v_max() - d.v_min()) * (f64::from(j) / 2.0),
            );
            match s.evaluate_point(k, uv) {
                Ok(p) => {
                    o.push(0.0);
                    put_vec(o, p);
                }
                Err(e) => o.push(status_code(&e)),
            }
        }
    }
    let nurb = match s.kind() {
        SurfaceKind::BSpline(b) => b.gw_nurb(),
        SurfaceKind::SurfOfRevolution(r) => r.base().gw_nurb(),
        _ => return,
    };
    let Some(sur) = nurb else {
        o.push(0.0);
        return;
    };
    o.push(1.0);
    let ix = sur.indices();
    o.extend([ix.n, ix.m, ix.p, ix.q, ix.r, ix.s].map(|x| x as f64));
    for row in sur.rows() {
        for p in row {
            o.extend([p.x, p.y, p.z, p.w]);
        }
    }
    o.extend_from_slice(sur.u_knots());
    o.extend_from_slice(sur.v_knots());
}

fn orient_code(o: OrientType) -> f64 {
    match o {
        OrientType::Unknown => 0.0,
        OrientType::Same => 1.0,
        OrientType::Opposite => 2.0,
    }
}

fn tsect_code(t: TsectCurveType) -> f64 {
    match t {
        TsectCurveType::Unknown => 0.0,
        TsectCurveType::Touching => 1.0,
        TsectCurveType::Crossing => 2.0,
        TsectCurveType::Tangent => 3.0,
        TsectCurveType::Coincident => 4.0,
        TsectCurveType::NearTangent => 5.0,
        TsectCurveType::RegionBoundary => 6.0,
    }
}

/// The B-spline part of a curve as a `CrvIn`, scaled about the origin and translated
/// (`None` for a curve that is not a B-spline kind).
fn curve_in_of(c: &SmCurve, d: [f64; 3], scale: f64) -> Option<CrvIn> {
    let base = match c.kind() {
        CurveKind::BSpline(b) => b,
        CurveKind::Line(l) => l.base(),
        _ => return None,
    };
    let n = base.gw_nurb()?;
    let dim = base.dim();
    let cpts = n
        .cpts()
        .iter()
        .map(|p| {
            let w = if p.w == NOW { 1.0 } else { p.w };
            let z = if dim == 2 || p.z == NOZ {
                NOZ
            } else {
                scale * p.z + w * d[2]
            };
            [scale * p.x + w * d[0], scale * p.y + w * d[1], z, p.w]
        })
        .collect();
    Some(CrvIn {
        dim,
        cpts,
        p: n.degree(),
        u: n.knots().to_vec(),
    })
}

// ---- resolution -----------------------------------------------------------------------------

/// Per B-rep `[vertices, edges, faces, edgeuses, faceuses]`.
type BrepCounts = [usize; 5];

struct Counts {
    breps: [Option<BrepCounts>; 2],
    /// Store curve indices by dimension: `[3D, 2D]`.
    curves: [Vec<usize>; 2],
    surfaces: usize,
}

impl Counts {
    fn of(c: BrepCounts, kind: u8) -> usize {
        match kind {
            K_VERTEX => c[0],
            K_EDGE | K_EDGE_CURVE => c[1],
            K_FACE | K_FACE_SURFACE => c[2],
            K_EDGEUSE | K_EU_UV => c[3],
            K_FACEUSE => c[4],
            _ => 0,
        }
    }

    /// Resolves a selector handle: `None` for null, `Some(Err)` when it names nothing.
    /// A store-curve selector is taken against the curves of dimension `dim` (3 or 2).
    fn resolve(&self, h: Hin, dim: usize) -> Result<Option<H>, ()> {
        let [b, kind, sel] = h;
        if kind == K_NULL || sel < 0 {
            return Ok(None);
        }
        let kind = u8::try_from(kind).map_err(|_| ())?;
        let sel = u64::try_from(sel).map_err(|_| ())?;
        match kind {
            K_BREP => {
                let slot = usize::try_from(b).map_err(|_| ())?;
                if self.breps.get(slot).copied().flatten().is_none() {
                    return Err(());
                }
                Ok(Some(hd(b as i8, K_BREP, slot as u32)))
            }
            K_CURVE => {
                let list = self.curves.get(usize::from(dim == 2)).ok_or(())?;
                if list.is_empty() {
                    return Ok(None);
                }
                let i = *list.get((sel % list.len() as u64) as usize).ok_or(())?;
                Ok(Some(hd(2, K_CURVE, i as u32)))
            }
            K_SURFACE => {
                if self.surfaces == 0 {
                    return Ok(None);
                }
                Ok(Some(hd(2, K_SURFACE, (sel % self.surfaces as u64) as u32)))
            }
            K_VERTEX | K_EDGE | K_FACE | K_EDGE_CURVE | K_FACE_SURFACE | K_EDGEUSE | K_FACEUSE
            | K_EU_UV => {
                let slot = usize::try_from(b).map_err(|_| ())?;
                let n = Self::of(self.breps.get(slot).copied().flatten().ok_or(())?, kind);
                if n == 0 {
                    // a B-rep without that kind (a wire has no faces): the null handle, as
                    // an empty store yields, so the op runs into its null guard
                    return Ok(None);
                }
                Ok(Some(hd(b as i8, kind, (sel % n as u64) as u32)))
            }
            _ => Err(()),
        }
    }
}

/// A resolved op: concrete handles (triples, `[-1, 8, -1]` null), explicit curves.
#[derive(Debug, Clone)]
struct ROp {
    op: OpIn,
}

/// The resolved header.
struct RHeader {
    local: Vec<[Hin; 2]>,
    shell: Vec<Hin>,
    pairs: Vec<[Hin; 2]>,
}

/// Resolves the header and every op of a case against the imported B-reps; `Err(name)`
/// names the first thing that cannot be resolved (both sides report it).
#[allow(clippy::too_many_lines)]
fn resolve(i: &Inputs, c: &mut Counts, two: &Two) -> Result<(RHeader, Vec<ROp>), String> {
    let hk = |c: &Counts,
              x: Hin,
              kinds: &[u8],
              allow_null: bool,
              dim: usize,
              what: &str|
     -> Result<Hin, String> {
        let r = c
            .resolve(x, dim)
            .map_err(|()| format!("unresolvable_{what}_handle"))?;
        match r {
            None if allow_null => Ok(h_to_triple(None)),
            None => Err(format!("unresolvable_{what}_null")),
            Some(r) if kinds.contains(&r.kind) => Ok(h_to_triple(Some(r))),
            Some(_) => Err(format!("unresolvable_{what}_kind")),
        }
    };
    // header: the FFI refuses a negative tolerance (no upstream caller passes one); the
    // refusal is mirrored here, as the B-rep guards are, so that neither side runs the
    // script
    if i.approx < 0.0 {
        return Err("approx_negative".into());
    }
    // (the local pairs come last: their store surfaces are named against the store the
    // ops fill, and resolved again at each local op)
    let mut shell = Vec::new();
    for (j, f) in i.shell.iter().enumerate() {
        shell.push(hk(c, *f, &[K_FACE], true, 3, &format!("shell{j}"))?);
    }
    // (the relation is many-to-many: an edge use mapped to two different offset edge
    // uses makes GetOffsetEdgeuse give null for it; a repeated pair adds nothing)
    let mut pairs = Vec::new();
    for (j, [a, b]) in i.pairs.iter().enumerate() {
        let what = format!("pair{j}");
        let a = hk(c, *a, &[K_EDGEUSE], false, 3, &what)?;
        let b = hk(c, *b, &[K_EDGEUSE], false, 3, &what)?;
        pairs.push([a, b]);
    }
    // ops
    let mut out = Vec::new();
    for (idx, op) in i.ops.iter().enumerate() {
        let what = format!("op{idx}");
        let h = |c: &Counts, x: Hin, kinds: &[u8], allow_null: bool, dim: usize| {
            hk(c, x, kinds, allow_null, dim, &what)
        };
        let eu =
            |c: &Counts, x: Hin, allow_null: bool| hk(c, x, &[K_EDGEUSE], allow_null, 3, &what);
        let rop = match op {
            OpIn::AddCurve { curve } => {
                let dim = usize::from(curve.dim == 2);
                let n = c.curves[0].len() + c.curves[1].len();
                if let Some(list) = c.curves.get_mut(dim) {
                    list.push(n);
                }
                OpIn::AddCurve {
                    curve: curve.clone(),
                }
            }
            OpIn::AddCurveFrom { src, d, scale } => {
                // a null source, or one without a (B-spline) curve, has nothing to copy:
                // the op is dropped from the script both sides run
                let s = h(c, *src, &[K_EDGE, K_EDGEUSE], true, 3)?;
                let Some(sh) = triple_to_h(s) else { continue };
                let source = if sh.kind == K_EDGE {
                    two.edge_curve_h(sh).ok().flatten()
                } else {
                    two.eu_uv_h(sh).ok().flatten()
                };
                let Some(curve) = source
                    .and_then(|ch| ClassGeometry::curve(two, ch))
                    .and_then(|cv| curve_in_of(cv, *d, *scale))
                else {
                    continue;
                };
                let dim = usize::from(curve.dim == 2);
                let n = c.curves[0].len() + c.curves[1].len();
                if let Some(list) = c.curves.get_mut(dim) {
                    list.push(n);
                }
                OpIn::AddCurve { curve }
            }
            OpIn::AddSurface { surface } => {
                c.surfaces += 1;
                OpIn::AddSurface {
                    surface: surface.clone(),
                }
            }
            OpIn::SetEdgeCurve { e, c: cc } => OpIn::SetEdgeCurve {
                e: h(c, *e, &[K_EDGE], true, 3)?,
                c: h(c, *cc, &[K_CURVE], true, 3)?,
            },
            OpIn::SetEuUv {
                eu: e,
                c: cc,
                max_dist,
            } => OpIn::SetEuUv {
                eu: eu(c, *e, true)?,
                c: h(c, *cc, &[K_CURVE], true, 2)?,
                max_dist: *max_dist,
            },
            OpIn::Mark { eu: e } => OpIn::Mark {
                eu: eu(c, *e, true)?,
            },
            OpIn::Getters => OpIn::Getters,
            OpIn::FaceuseOffset { fu, skip } => OpIn::FaceuseOffset {
                fu: h(c, *fu, &[K_FACEUSE], true, 3)?,
                skip: *skip,
            },
            OpIn::LocalFaceuseOffset { fu, skip } => OpIn::LocalFaceuseOffset {
                fu: h(c, *fu, &[K_FACEUSE], true, 3)?,
                skip: *skip,
            },
            OpIn::VertexOffset { v, edges } => OpIn::VertexOffset {
                v: h(c, *v, &[K_VERTEX], true, 3)?,
                edges: edges
                    .iter()
                    .map(|e| h(c, *e, &[K_EDGE], true, 3))
                    .collect::<Result<Vec<_>, _>>()?,
            },
            OpIn::UpdateFaceDomain { f, s } => OpIn::UpdateFaceDomain {
                f: h(c, *f, &[K_FACE], true, 3)?,
                s: h(c, *s, &[K_SURFACE, K_FACE_SURFACE], true, 3)?,
            },
            OpIn::ReplaceEdgeCurves {
                eu: e,
                c3,
                uv,
                orig,
            } => OpIn::ReplaceEdgeCurves {
                eu: eu(c, *e, true)?,
                c3: h(c, *c3, &[K_CURVE], true, 3)?,
                uv: h(c, *uv, &[K_CURVE], true, 2)?,
                orig: eu(c, *orig, true)?,
            },
            OpIn::TrimExtendedEdge {
                base,
                other_base,
                off,
                other_off,
                c3,
                uv_base,
                uv_other,
            } => OpIn::TrimExtendedEdge {
                base: eu(c, *base, true)?,
                other_base: eu(c, *other_base, true)?,
                off: eu(c, *off, true)?,
                other_off: eu(c, *other_off, true)?,
                c3: h(c, *c3, &[K_CURVE, K_EDGE_CURVE], true, 3)?,
                uv_base: h(c, *uv_base, &[K_CURVE], true, 2)?,
                uv_other: h(c, *uv_other, &[K_CURVE], true, 2)?,
            },
            OpIn::ExtendTrimEdges {
                eu: e,
                other,
                off,
                other_off,
            } => OpIn::ExtendTrimEdges {
                eu: eu(c, *e, true)?,
                other: eu(c, *other, true)?,
                off: eu(c, *off, true)?,
                other_off: eu(c, *other_off, true)?,
            },
            OpIn::MateEdges {
                eu: e,
                other,
                off,
                other_off,
            } => OpIn::MateEdges {
                eu: eu(c, *e, true)?,
                other: eu(c, *other, true)?,
                off: eu(c, *off, true)?,
                other_off: eu(c, *other_off, true)?,
            },
            OpIn::HaveTangentGaps { n, off, other_off } => OpIn::HaveTangentGaps {
                n: (*n).min(ffi::MAX_SAMPLES as u64),
                off: eu(c, *off, true)?,
                other_off: eu(c, *other_off, true)?,
            },
            OpIn::EdgeuseOffset {
                orig,
                other_orig,
                off,
                other_off,
            } => OpIn::EdgeuseOffset {
                orig: eu(c, *orig, true)?,
                other_orig: eu(c, *other_orig, true)?,
                off: eu(c, *off, true)?,
                other_off: eu(c, *other_off, true)?,
            },
            OpIn::EdgeState { e } => OpIn::EdgeState {
                e: h(c, *e, &[K_EDGE], true, 3)?,
            },
            OpIn::SetEdgeTol { e, tol } => OpIn::SetEdgeTol {
                e: h(c, *e, &[K_EDGE], true, 3)?,
                tol: tol.abs(),
            },
        };
        out.push(ROp { op: rop });
    }
    let mut local = Vec::new();
    for (j, [f, s]) in i.local.iter().enumerate() {
        let what = format!("local{j}");
        local.push([
            hk(c, *f, &[K_FACE], true, 3, &what)?,
            hk(c, *s, &[K_SURFACE], true, 3, &what)?, // a face's own surface is refused (the FFI's contract)
        ]);
    }
    Ok((
        RHeader {
            local,
            shell,
            pairs,
        },
        out,
    ))
}

fn push_h(v: &mut Vec<f64>, h: Hin) {
    v.extend([h[0] as f64, h[1] as f64, h[2] as f64]);
}

/// The shim's RUN input.
fn encode(i: &Inputs, hdr: &RHeader, ops: &[ROp]) -> Result<Vec<f64>, HarnessError> {
    let mut v = blocks(i)?;
    v.extend([i.dist, i.approx, flag(i.analytics)]);
    v.push(hdr.local.len() as f64);
    for [f, s] in &hdr.local {
        push_h(&mut v, *f);
        push_h(&mut v, *s);
    }
    v.extend([flag(i.inset), flag(i.extend)]);
    v.push(hdr.shell.len() as f64);
    for f in &hdr.shell {
        push_h(&mut v, *f);
    }
    v.push(hdr.pairs.len() as f64);
    for [a, b] in &hdr.pairs {
        push_h(&mut v, *a);
        push_h(&mut v, *b);
    }
    v.push(ops.len() as f64);
    for r in ops {
        match &r.op {
            OpIn::AddCurve { curve } => {
                v.push(1.0);
                push_curve(&mut v, curve)?;
            }
            OpIn::AddCurveFrom { .. } => return Err(harness("unresolved from-curve")),
            OpIn::AddSurface { surface } => {
                v.push(2.0);
                push_surface(&mut v, surface)?;
            }
            OpIn::SetEdgeCurve { e, c } => {
                v.push(3.0);
                push_h(&mut v, *e);
                push_h(&mut v, *c);
            }
            OpIn::SetEuUv { eu, c, max_dist } => {
                v.push(4.0);
                push_h(&mut v, *eu);
                push_h(&mut v, *c);
                v.push(*max_dist);
            }
            OpIn::Mark { eu } => {
                v.push(5.0);
                push_h(&mut v, *eu);
            }
            OpIn::Getters => v.push(6.0),
            OpIn::FaceuseOffset { fu, skip } => {
                v.push(7.0);
                push_h(&mut v, *fu);
                v.push(flag(*skip));
            }
            OpIn::VertexOffset { v: vh, edges } => {
                v.push(8.0);
                push_h(&mut v, *vh);
                v.push(edges.len() as f64);
                for e in edges {
                    push_h(&mut v, *e);
                }
            }
            OpIn::UpdateFaceDomain { f, s } => {
                v.push(9.0);
                push_h(&mut v, *f);
                push_h(&mut v, *s);
            }
            OpIn::ReplaceEdgeCurves { eu, c3, uv, orig } => {
                v.push(10.0);
                for h in [eu, c3, uv, orig] {
                    push_h(&mut v, *h);
                }
            }
            OpIn::TrimExtendedEdge {
                base,
                other_base,
                off,
                other_off,
                c3,
                uv_base,
                uv_other,
            } => {
                v.push(11.0);
                for h in [base, other_base, off, other_off, c3, uv_base, uv_other] {
                    push_h(&mut v, *h);
                }
            }
            OpIn::ExtendTrimEdges {
                eu,
                other,
                off,
                other_off,
            } => {
                v.push(12.0);
                for h in [eu, other, off, other_off] {
                    push_h(&mut v, *h);
                }
            }
            OpIn::MateEdges {
                eu,
                other,
                off,
                other_off,
            } => {
                v.push(13.0);
                for h in [eu, other, off, other_off] {
                    push_h(&mut v, *h);
                }
            }
            OpIn::HaveTangentGaps { n, off, other_off } => {
                v.push(14.0);
                v.push(*n as f64);
                push_h(&mut v, *off);
                push_h(&mut v, *other_off);
            }
            OpIn::EdgeuseOffset {
                orig,
                other_orig,
                off,
                other_off,
            } => {
                v.push(15.0);
                for h in [orig, other_orig, off, other_off] {
                    push_h(&mut v, *h);
                }
            }
            OpIn::LocalFaceuseOffset { fu, skip } => {
                v.push(16.0);
                push_h(&mut v, *fu);
                v.push(flag(*skip));
            }
            OpIn::EdgeState { e } => {
                v.push(17.0);
                push_h(&mut v, *e);
            }
            OpIn::SetEdgeTol { e, tol } => {
                v.push(18.0);
                push_h(&mut v, *e);
                v.push(*tol);
            }
        }
    }
    Ok(v)
}

// ---- running ----------------------------------------------------------------------------------

fn has_surface_record(type_code: i64) -> bool {
    matches!(
        type_code,
        SURF_OF_REVOLUTION_TYPE
            | SURF_OF_EXTRUSION_TYPE
            | CONE_TYPE
            | CYLINDER_TYPE
            | SPHERE_TYPE
            | TORUS_TYPE
    )
}

/// Restores supported concrete face surfaces from native input records. Keep the
/// graph's owner identity and require a complete, type-correct record for every
/// supported subclass rather than silently falling back to its B-spline part.
fn read_face_surfaces(
    b: &mut Brep,
    exact: &mut [bool],
    types: &[Option<i64>],
    records: &[Option<Vec<u8>>],
) -> Result<(), HarnessError> {
    if records.len() != b.faces().len()
        || exact.len() != records.len()
        || types.len() != records.len()
    {
        return Err(harness("surface records do not match the native face list"));
    }
    let k = Kernel::default();
    for (i, (rec, type_code)) in records.iter().zip(types).enumerate() {
        let Some(bytes) = rec else {
            if type_code.is_some_and(has_surface_record) {
                return Err(harness(format!(
                    "missing concrete surface record of face {i}"
                )));
            }
            continue;
        };
        let type_code = type_code
            .filter(|&ty| has_surface_record(ty))
            .ok_or_else(|| harness(format!("unexpected concrete surface record of face {i}")))?;
        let f = FaceId::from_index(u32::try_from(i).map_err(|_| harness("face index"))?);
        let mut db = DatabaseReader::with_stream(FileType::Binary, InputStream::new(bytes.clone()));
        let mut surface = Surface::read_from_db(&k, type_code, &mut db, SURFACE_DB_VERSION)
            .map_err(|e| harness(format!("surface record of face {i}: {e}")))?;
        if surface.type_code() != type_code || db.in_stream().is_none_or(|s| s.remaining() != 0) {
            return Err(harness(format!(
                "surface record type/length mismatch at face {i}"
            )));
        }
        let face = b.face_mut(f).map_err(|e| harness(e.to_string()))?;
        let owner = face
            .surface
            .as_ref()
            .ok_or_else(|| harness("surface record on a null face surface"))?
            .owner();
        surface.set_owner(owner);
        face.surface = Some(surface);
        // Replacement bypasses add_face_record's cache synchronization. Restore
        // that invariant from this face's real tolerance, never a guessed default.
        b.sync_face_surface_tolerance(f)
            .map_err(|e| harness(e.to_string()))?;
        *exact
            .get_mut(i)
            .ok_or_else(|| harness("surface record past the faces"))? = true;
    }
    Ok(())
}

struct Prepared {
    two: Two,
    hdr: RHeader,
    ops: Vec<ROp>,
}

/// Exports and imports the B-reps and resolves the script; `Err(Ok(output))` is the
/// output both sides report when the B-reps cannot be built or something cannot be
/// resolved.
fn prepare(i: &Inputs) -> Result<Prepared, Box<Result<Output, HarnessError>>> {
    let ex = export(i).map_err(|e| Box::new(Err(e)))?;
    let Some((a, b)) = ffi::split_exports(&ex) else {
        return Err(Box::new(Ok(build_failure(&ex))));
    };
    let [surfaces_a, surfaces_b] =
        ffi::split_surface_records(&ex).ok_or_else(|| Box::new(Err(harness("surface records"))))?;
    let (mut brep_a, mut exact_a, types_a) = import_with_types(&a).map_err(|e| Box::new(Err(e)))?;
    read_face_surfaces(&mut brep_a, &mut exact_a, &types_a, &surfaces_a)
        .map_err(|e| Box::new(Err(e)))?;
    let (brep_b, exact_b) = match &b {
        Some(b) => {
            let (mut brep, mut exact, types) =
                import_with_types(b).map_err(|e| Box::new(Err(e)))?;
            read_face_surfaces(&mut brep, &mut exact, &types, &surfaces_b)
                .map_err(|e| Box::new(Err(e)))?;
            (Some(brep), exact)
        }
        None => (None, Vec::new()),
    };
    let count5 = |b: &Brep| {
        [
            b.vertices().len(),
            b.edges().len(),
            b.faces().len(),
            b.edgeuses().len(),
            b.faceuses().len(),
        ]
    };
    let mut counts = Counts {
        breps: [Some(count5(&brep_a)), brep_b.as_ref().map(count5)],
        curves: [Vec::new(), Vec::new()],
        surfaces: 0,
    };
    let two = Two::new(brep_a, brep_b, [exact_a, exact_b]);
    let (hdr, ops) = match resolve(i, &mut counts, &two) {
        Ok(r) => r,
        Err(name) => return Err(Box::new(Ok(Output::err(name)))),
    };
    Ok(Prepared { two, hdr, ops })
}

fn run_cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let p = match prepare(i) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    let input = encode(i, &p.hdr, &p.ops)?;
    let pa = path_of(Some(&i.brep_a))?;
    let pb = path_of(i.brep_b.as_ref())?;
    let out = ffi::eval(Op::Run, [&pa, &pb], &input).map_err(|e| harness(format!("c++: {e}")))?;
    if out.first() != Some(&0.0) {
        return Ok(build_failure(&out));
    }
    Ok(Output::new().values("state", out))
}

// ---- the script, Rust side --------------------------------------------------------------

/// A failure of the Rust side: an error to report or an unported callee.
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

/// The status of a result (`0` success, the upstream code of an upstream error);
/// `None` with [`NULL_DEREF`] pushed for undefined behaviour upstream; `Err` for
/// everything else (unported callees are reported by the caller).
fn status<T>(r: Result<T, SolidError>, o: &mut Vec<f64>) -> Result<Option<T>, Fail> {
    match r {
        Ok(v) => {
            o.push(0.0);
            Ok(Some(v))
        }
        Err(SolidError::Upstream { code, .. }) => {
            o.push(f64::from(code));
            Ok(None)
        }
        Err(SolidError::InvalidInput {
            operation: UNDEFINED_UPSTREAM,
            ..
        }) => {
            // the library met a null dereference or failed cast upstream would make
            // (undefined): the shim's refusal status, so that it never passes as a
            // real SM_ERR_INVALID_INPUT; a shim that did not refuse the same op fails
            o.push(NULL_DEREF);
            Ok(None)
        }
        Err(SolidError::InvalidInput { .. }) => {
            // upstream's SM_ERR_INVALID_INPUT status of a callee
            o.push(SM_ERR_INVALID_INPUT);
            Ok(None)
        }
        Err(e) => Err(Fail::Solid(e)),
    }
}

fn put_orients(o: &mut Vec<f64>, v: &[OrientType]) {
    o.push(v.len() as f64);
    for &x in v {
        o.push(orient_code(x));
    }
}

fn put_hcurves(o: &mut Vec<f64>, two: &Two, hs: &[H]) -> Result<(), HarnessError> {
    o.push(hs.len() as f64);
    for &h in hs {
        put_curve(o, ClassGeometry::curve(two, h))?;
    }
    Ok(())
}

fn put_ssi(o: &mut Vec<f64>, two: &Two, s: &SsiData<H>) -> Result<(), HarnessError> {
    o.push(s.curves_3d.len() as f64);
    for (j, &c) in s.curves_3d.iter().enumerate() {
        put_curve(o, ClassGeometry::curve(two, c))?;
        put_curve(
            o,
            s.uv_curves1
                .get(j)
                .copied()
                .flatten()
                .and_then(|h| ClassGeometry::curve(two, h)),
        )?;
        put_curve(
            o,
            s.uv_curves2
                .get(j)
                .copied()
                .flatten()
                .and_then(|h| ClassGeometry::curve(two, h)),
        )?;
        o.push(s.curve_types.get(j).map_or(-1.0, |t| tsect_code(*t)));
        o.push(s.deviations.get(j).copied().unwrap_or(-1.0));
    }
    Ok(())
}

/// Whether `ReplaceEdgeCurves(eu, c3, NULL, orig)` would reverse the curves (the shim's
/// replica of the function's shape test).
fn would_reverse(k: &Kernel, two: &Two, eu: H, c3: H, orig: Option<H>) -> Result<bool, Fail> {
    let edge = two
        .edgeuse_edge(eu)?
        .ok_or_else(|| invalid("edge use without edge"))?;
    let euc = two
        .edge_curve(edge)?
        .ok_or_else(|| invalid("edge without curve"))?;
    let ivl = two.edge_interval(edge)?;
    let tol = two.edge_tolerance(edge)?;
    let cv = |h: H| ClassGeometry::curve(two, h).ok_or_else(|| invalid("curve"));
    let es = cv(euc)?.evaluate_point(k, ivl.min())?;
    let ee = cv(euc)?.evaluate_point(k, ivl.max())?;
    let civl = cv(c3)?.natural_interval();
    let cs = cv(c3)?.evaluate_point(k, civl.min())?;
    let ce = cv(c3)?.evaluate_point(k, civl.max())?;
    if es.distance(cs) < tol && ee.distance(ce) < tol {
        return Ok(false);
    }
    if es.distance(ce) < tol && ee.distance(cs) < tol {
        return Ok(false);
    }
    let pv = match orig {
        None => cv(euc)?.evaluate(k, ivl.evaluate(0.5), 1, true, true)?,
        Some(orig) => {
            let oe = two
                .edgeuse_edge(orig)?
                .ok_or_else(|| invalid("edge use without edge"))?;
            let oc = two
                .edge_curve(oe)?
                .ok_or_else(|| invalid("edge without curve"))?;
            let oivl = two.edge_interval(oe)?;
            cv(oc)?.evaluate(k, oivl.evaluate(0.5), 1, true, true)?
        }
    };
    let pv2 = cv(c3)?.evaluate(k, civl.evaluate(0.5), 1, true, true)?;
    let t1 = pv.get(1).copied().ok_or_else(|| invalid("tangent"))?;
    let t2 = pv2.get(1).copied().ok_or_else(|| invalid("tangent"))?;
    Ok(t1.dot(t2) < 0.0)
}

/// Whether a UV curve is a degree-1 B-spline (or line) along a constant `u` or `v`: the
/// UV curves `sm_ExtractIsoCurve` lifts to the surface's iso curve (their extensions in
/// `ExtendTrimEdges` stay such lines). The bound is generous, so a borderline curve
/// is counted as iso.
fn is_uv_iso_segment(c: &SmCurve) -> bool {
    let nurb = match c.kind() {
        CurveKind::BSpline(b) => b.gw_nurb().cloned(),
        CurveKind::Line(l) => {
            let mut l = l.clone();
            if l.base().gw_nurb().is_none() && l.make_nurb().is_err() {
                return false;
            }
            l.base().gw_nurb().cloned()
        }
        _ => return false,
    };
    let Some(n) = nurb else {
        return false;
    };
    if n.degree() > 1 {
        return false;
    }
    let pts: Vec<(f64, f64)> = n
        .cpts()
        .iter()
        .map(|p| {
            if p.w == NOW {
                (p.x, p.y)
            } else {
                (p.x / p.w, p.y / p.w)
            }
        })
        .collect();
    let flat = |f: fn(&(f64, f64)) -> f64| {
        let (lo, hi) = pts
            .iter()
            .map(f)
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), x| {
                (l.min(x), h.max(x))
            });
        hi - lo <= 1e-9 * (1.0 + lo.abs().max(hi.abs()))
    };
    flat(|p| p.0) || flat(|p| p.1)
}

/// Whether the curve is of a B-spline kind (`IsKindOf(SmBSplineCurve_TYPE)`).
fn is_bspline_kind(c: &SmCurve) -> bool {
    c.is_kind_of(BSPLINE_CURVE_TYPE)
}

#[allow(clippy::too_many_lines)]
fn run_rust_script(i: &Inputs, p: Prepared) -> Result<Vec<f64>, Fail> {
    let k = Kernel::default();
    let Prepared { mut two, hdr, ops } = p;
    let mut o = Vec::new();
    o.push(0.0);

    let mut geom = OffsetGeometryCreation::new(i.dist, i.approx);
    geom.set_make_analytics(i.analytics);
    // the local creation: the listed faces, each with its store surface as the store
    // stands at the op (the shim resolves the handles again at each local op). Its
    // analytics flag stays FALSE, as upstream's constructor leaves it (neither side
    // calls SetMakeAnalytics on it), so the listed surface is returned as is.
    let local_faces: Vec<Option<H>> = hdr.local.iter().map(|[f, _]| triple_to_h(*f)).collect();
    let local_lists = |two: &Two| -> (Vec<H>, Vec<H>) {
        let mut lf = Vec::new();
        let mut ls = Vec::new();
        for [f, sh] in &hdr.local {
            let (Some(f), Some(sh)) = (triple_to_h(*f), triple_to_h(*sh)) else {
                continue;
            };
            if ClassGeometry::surface(two, sh).is_none() {
                continue;
            }
            lf.push(f);
            ls.push(sh);
        }
        (lf, ls)
    };
    let (lf0, ls0) = local_lists(&two);
    let local = LocalOperationGeometryCreation::new(i.dist, i.approx, &lf0, &ls0);
    let shell_count = hdr.shell.len();
    let mut exec = Exec {
        inner: OffsetExecutivePort::with_geometry_parameters(
            OffsetOperation::SolidOffset,
            geom.offset_distance(),
            geom.this_approx_tol_3d(),
            geom.tangency_tol_radians(),
        ),
        seeded: BTreeSet::new(),
        made: BTreeMap::new(),
    };
    exec.inner.set_inset(i.inset);
    exec.inner.set_extend_convex_edges(i.extend);
    let mut shell = Vec::new();
    for f in hdr.shell.iter().filter_map(|h| triple_to_h(*h)) {
        shell.push(FaceId::from_index(exec_index(f)?));
    }
    exec.inner.set_shell_faces(&shell);
    for [a, b] in &hdr.pairs {
        if let (Some(a), Some(b)) = (triple_to_h(*a), triple_to_h(*b)) {
            exec.inner.relate_edgeuses(
                EdgeuseId::from_index(exec_index(a)?),
                EdgeuseId::from_index(exec_index(b)?),
            );
        }
    }
    let all_ok = two.all_ok();
    let mark = MarkType::Mark;

    for r in &ops {
        exec.seed(&two);
        match &r.op {
            OpIn::AddCurve { curve } => {
                o.push(1.0);
                two.curves.push(rust_curve(curve)?);
                o.push(0.0);
                o.push((two.curves.len() - 1) as f64);
            }
            OpIn::AddCurveFrom { .. } => return Err(harness("unresolved from-curve").into()),
            OpIn::AddSurface { surface } => {
                o.push(2.0);
                two.surfaces.push(Some(rust_surface(surface)?));
                o.push(0.0);
                o.push((two.surfaces.len() - 1) as f64);
            }
            OpIn::SetEdgeCurve { e, c } => {
                o.push(3.0);
                let (Some(e), Some(c)) = (triple_to_h(*e), triple_to_h(*c)) else {
                    o.push(NULL_DEREF);
                    continue;
                };
                let curve = ClassGeometry::curve(&two, c)
                    .ok_or_else(|| harness("store curve"))?
                    .clone();
                let natural = curve.natural_interval();
                two.set_edge_curve_from(e, curve)?;
                two.set_edge_interval(e, natural)?;
                o.push(0.0);
                two.put_edge_state(&mut o, e)?;
            }
            OpIn::SetEuUv { eu, c, max_dist } => {
                o.push(4.0);
                let Some(eu) = triple_to_h(*eu) else {
                    o.push(NULL_DEREF);
                    continue;
                };
                let curve = match triple_to_h(*c) {
                    Some(c) => Some(
                        ClassGeometry::curve(&two, c)
                            .ok_or_else(|| harness("store curve"))?
                            .clone(),
                    ),
                    None => None,
                };
                two.set_eu_uv_from(eu, curve, *max_dist)?;
                o.push(0.0);
                let uv = two.eu_uv_h(eu)?;
                put_curve(&mut o, uv.and_then(|h| ClassGeometry::curve(&two, h)))?;
            }
            OpIn::Mark { eu } => {
                o.push(5.0);
                let Some(eu) = triple_to_h(*eu) else {
                    o.push(NULL_DEREF);
                    continue;
                };
                two.marked.insert(eu);
                o.push(0.0);
                o.push(1.0);
            }
            OpIn::Getters => {
                o.push(6.0);
                o.push(0.0);
                o.extend([
                    geom.this_approx_tol_3d(),
                    geom.tangency_tol_radians(),
                    geom.tangency_tol_degrees(),
                    geom.offset_distance(),
                    local.base.this_approx_tol_3d(),
                    local.base.tangency_tol_degrees(),
                    local.base.offset_distance(),
                    geom.tangency_tol_degrees(), // the executive's GetTangencyTolDegrees reads the creation's
                    flag(exec.inset()),
                    flag(exec.extend_convex_edges()),
                    shell_count as f64,
                ]);
            }
            OpIn::FaceuseOffset { fu, skip } | OpIn::LocalFaceuseOffset { fu, skip } => {
                let is_local = matches!(r.op, OpIn::LocalFaceuseOffset { .. });
                o.push(if is_local { 16.0 } else { 7.0 });
                let Some(fu) = triple_to_h(*fu) else {
                    o.push(NULL_DEREF);
                    continue;
                };
                let Some(face) = two.faceuse_face(fu)? else {
                    o.push(NULL_DEREF);
                    continue;
                };
                let Some(surface) = two.face_surface(face)? else {
                    o.push(NULL_DEREF);
                    continue;
                };
                if is_local {
                    // a listed face without surface (not added yet, or deleted) is
                    // dereferenced upstream
                    if let Some(idx) = local_faces.iter().position(|f| *f == Some(face)) {
                        let sh = hdr.local.get(idx).and_then(|[_, sh]| triple_to_h(*sh));
                        if sh.is_none_or(|sh| ClassGeometry::surface(&two, sh).is_none()) {
                            o.push(NULL_DEREF);
                            continue;
                        }
                    }
                }
                if !is_local {
                    if two.face_exact(face) != Some(true) {
                        return Err(Fail::Solid(SolidError::Unported {
                            upstream: "SmSurface subclass::CreateOffsetSurface (face surface imported as its B-spline part)",
                            unit: ANALYTIC_EXPORT,
                        }));
                    }
                    if !skip
                        && ClassGeometry::surface(&two, surface)
                            .is_some_and(|s| s.type_code() == SURF_OF_REVOLUTION_TYPE)
                    {
                        return Err(Fail::Solid(SolidError::Unported {
                            upstream: "FaceuseOffset self-intersection of a surface of revolution (HANG-26)",
                            unit: HANG_26,
                        }));
                    }
                }
                let r = if is_local {
                    let (lf, ls) = local_lists(&two);
                    let local_now = LocalOperationGeometryCreation::new(i.dist, i.approx, &lf, &ls);
                    local_now.faceuse_offset(&k, &mut two, fu, *skip)
                } else {
                    geom.faceuse_offset(&k, &mut two, fu, *skip)
                };
                let Some((ns, ssi)) = status(r, &mut o)? else {
                    continue;
                };
                o.push(flag(ns.is_some()));
                if let Some(ns) = ns {
                    put_srf(&k, &mut o, ClassGeometry::surface(&two, ns));
                }
                put_ssi(&mut o, &two, &ssi)?;
            }
            OpIn::VertexOffset { v, edges } => {
                o.push(8.0);
                let v = triple_to_h(*v);
                let edges: Vec<Option<H>> = edges.iter().map(|e| triple_to_h(*e)).collect();
                let mut bad = false;
                let mut es: Vec<H> = Vec::new();
                for e in &edges {
                    match e {
                        Some(e) if two.edge_curve(*e)?.is_some() => es.push(*e),
                        _ => bad = true,
                    }
                }
                if !bad && es.len() == 1 {
                    let e = es.first().copied().ok_or_else(|| harness("edge"))?;
                    let faces = two.edge_faces(e)?;
                    let curve_h = two.edge_curve(e)?.ok_or_else(|| invalid("curve"))?;
                    let is_bs = ClassGeometry::curve(&two, curve_h).is_some_and(is_bspline_kind);
                    match faces.first() {
                        Some(f) if two.face_surface(*f)?.is_some() && is_bs => {}
                        _ => bad = true,
                    }
                }
                // (no `all_ok`: VertexOffset walks no loops, as the shim)
                if bad {
                    o.push(NULL_DEREF);
                    continue;
                }
                let r = geom.vertex_offset(&k, &mut two, v, &es);
                let Some(out) = status(r, &mut o)? else {
                    continue;
                };
                put_hcurves(&mut o, &two, &out.trimming_curves)?;
                put_orients(&mut o, &out.orients);
                o.push(flag(out.surface.is_some()));
                if let Some(s) = out.surface {
                    put_srf(&k, &mut o, ClassGeometry::surface(&two, s));
                }
            }
            OpIn::UpdateFaceDomain { f, s } => {
                o.push(9.0);
                let (Some(f), Some(s)) = (triple_to_h(*f), triple_to_h(*s)) else {
                    o.push(NULL_DEREF);
                    continue;
                };
                if two.face_surface(f)?.is_none() || ClassGeometry::surface(&two, s).is_none() {
                    o.push(NULL_DEREF);
                    continue;
                }
                if !two
                    .face_exact(f)
                    .ok_or_else(|| Fail::Harness(harness("face outside the import table")))?
                {
                    // the trim-curve drops and GetNaturalUVDomain are virtual on the face's
                    // surface, which the record also compares afterwards: a face imported as
                    // its B-spline part dispatches (and reports) the B-spline kind instead
                    return Err(Fail::Solid(SolidError::Unported {
                        upstream: "UpdateFaceSurfaceDomain on a face surface imported as its B-spline part",
                        unit: ANALYTIC_EXPORT,
                    }));
                }
                let r = geom.update_face_surface_domain(&k, &mut two, f, s);
                status(r, &mut o)?; // the face's domain and surface: after every status
                let d = two.face_uv_domain(f)?;
                o.extend([d.u_min(), d.u_max(), d.v_min(), d.v_max()]);
                let sh = two.face_surface(f)?;
                put_srf(&k, &mut o, sh.and_then(|h| ClassGeometry::surface(&two, h)));
            }
            OpIn::ReplaceEdgeCurves { eu, c3, uv, orig } => {
                o.push(10.0);
                let (eu, c3, uv, orig) = (
                    triple_to_h(*eu),
                    triple_to_h(*c3),
                    triple_to_h(*uv),
                    triple_to_h(*orig),
                );
                let Some(eu) = eu else {
                    o.push(NULL_DEREF);
                    continue;
                };
                let Some(edge) = two.edgeuse_edge(eu)? else {
                    o.push(NULL_DEREF);
                    continue;
                };
                let mut bad = false;
                if let Some(c3) = c3 {
                    bad = two.edge_curve(edge)?.is_none() || two.edgeuse_vertex(eu)?.is_none();
                    if !bad {
                        match two.edgeuse_mate(eu)? {
                            Some(m) => bad = two.edgeuse_vertex(m)?.is_none(),
                            None => bad = true,
                        }
                    }
                    if !bad {
                        if let Some(orig) = orig {
                            bad = match two.edgeuse_edge(orig)? {
                                Some(oe) => two.edge_curve(oe)?.is_none(),
                                None => true,
                            };
                        }
                    }
                    if !bad && uv.is_none() && would_reverse(&k, &two, eu, c3, orig)? {
                        bad = true;
                    }
                }
                if bad {
                    o.push(NULL_DEREF);
                    continue;
                }
                // the copies the shim hands over
                let c3c = match c3 {
                    Some(h) => {
                        let c = ClassGeometry::curve(&two, h)
                            .ok_or_else(|| harness("store curve"))?
                            .clone();
                        Some(two.insert_curve(c).ok_or_else(|| harness("store"))?)
                    }
                    None => None,
                };
                let uvc = match uv {
                    Some(h) => {
                        let c = ClassGeometry::curve(&two, h)
                            .ok_or_else(|| harness("store curve"))?
                            .clone();
                        Some(two.insert_curve(c).ok_or_else(|| harness("store"))?)
                    }
                    None => None,
                };
                let r = geom.replace_edge_curves(&k, &mut two, eu, c3c, uvc, orig);
                if let Some(flags) = status(r, &mut o)? {
                    // the out-parameters: only after success
                    o.push(flag(flags.delete_3d_curve));
                    o.push(flag(flags.delete_uv_curve));
                }
                two.put_edge_state(&mut o, edge)?; // the edge's state: after every status
            }
            OpIn::TrimExtendedEdge {
                base,
                other_base,
                off,
                other_off,
                c3,
                uv_base,
                uv_other,
            } => {
                o.push(11.0);
                let (Some(base), Some(c3)) = (triple_to_h(*base), triple_to_h(*c3)) else {
                    o.push(NULL_DEREF);
                    continue;
                };
                if !all_ok {
                    o.push(NULL_DEREF);
                    continue;
                }
                let c3 = match c3.kind {
                    K_EDGE_CURVE => two.edge_curve_h(c3)?.ok_or_else(|| invalid("edge curve"))?,
                    _ => curve_h(
                        c3.b,
                        c3.kind,
                        c3.i,
                        ClassGeometry::curve(&two, c3).ok_or_else(|| invalid("curve"))?,
                    ),
                };
                let uvb = triple_to_h(*uv_base).and_then(|h| {
                    ClassGeometry::curve(&two, h).map(|c| curve_h(h.b, h.kind, h.i, c))
                });
                let uvo = triple_to_h(*uv_other).and_then(|h| {
                    ClassGeometry::curve(&two, h).map(|c| curve_h(h.b, h.kind, h.i, c))
                });
                let r = geom.trim_extended_edge(
                    &k,
                    &mut two,
                    &mut exec,
                    base,
                    triple_to_h(*other_base),
                    triple_to_h(*off),
                    triple_to_h(*other_off),
                    c3,
                    uvb,
                    uvo,
                    mark,
                );
                let Some(ivl) = status(r, &mut o)? else {
                    continue;
                };
                o.push(flag(ivl.is_some()));
                if let Some(ivl) = ivl {
                    o.extend([ivl.min(), ivl.max()]);
                }
            }
            OpIn::ExtendTrimEdges {
                eu,
                other,
                off,
                other_off,
            } => {
                o.push(12.0);
                let (eu, other, off, other_off) = (
                    triple_to_h(*eu),
                    triple_to_h(*other),
                    triple_to_h(*off),
                    triple_to_h(*other_off),
                );
                let (Some(eu), Some(off)) = (eu, off) else {
                    o.push(NULL_DEREF);
                    continue;
                };
                let mut bad = !all_ok || two.edgeuse_uv_trim_curve(eu)?.is_none();
                if !bad {
                    let edge = two.edgeuse_edge(eu)?.ok_or_else(|| invalid("edge"))?;
                    if !two.edge_is_lamina(edge)? {
                        match two.edgeuse_radial(eu)? {
                            Some(rad) => bad = two.edgeuse_uv_trim_curve(rad)?.is_none(),
                            None => bad = true,
                        }
                    }
                }
                if bad {
                    o.push(NULL_DEREF);
                    continue;
                }
                // LiftCurve's iso curve keeps the subclass's kind (lifts_iso_on_inexact_face)
                if two.lifts_iso_on_inexact_face(eu, off)? {
                    return Err(Fail::Solid(SolidError::Unported {
                        upstream: "SmSurface::LiftCurve iso curve of a subclass (face surface imported as its B-spline part)",
                        unit: ANALYTIC_EXPORT,
                    }));
                }
                let r = geom.extend_trim_edges(
                    &k,
                    &mut two,
                    &mut exec,
                    eu,
                    other,
                    Some(off),
                    other_off,
                    mark,
                );
                status(r, &mut o)?; // the edges' states: after every status
                let off_edge = two.edgeuse_edge(off)?.ok_or_else(|| invalid("edge"))?;
                two.put_edge_state(&mut o, off_edge)?;
                o.push(flag(other_off.is_some()));
                if let Some(oo) = other_off {
                    let e = two.edgeuse_edge(oo)?.ok_or_else(|| invalid("edge"))?;
                    two.put_edge_state(&mut o, e)?;
                }
            }
            OpIn::MateEdges {
                eu,
                other,
                off,
                other_off,
            } => {
                o.push(13.0);
                if !all_ok {
                    o.push(NULL_DEREF);
                    continue;
                }
                let (eu, other, off, other_off) = (
                    triple_to_h(*eu),
                    triple_to_h(*other),
                    triple_to_h(*off),
                    triple_to_h(*other_off),
                );
                let r = geom.mate_edges(&k, &mut two, &mut exec, eu, other, off, other_off, mark);
                status(r, &mut o)?; // the edges' states: after every status
                o.push(flag(off.is_some()));
                if let Some(off) = off {
                    let e = two.edgeuse_edge(off)?.ok_or_else(|| invalid("edge"))?;
                    two.put_edge_state(&mut o, e)?;
                }
                o.push(flag(other_off.is_some()));
                if let Some(oo) = other_off {
                    let e = two.edgeuse_edge(oo)?.ok_or_else(|| invalid("edge"))?;
                    two.put_edge_state(&mut o, e)?;
                }
            }
            OpIn::HaveTangentGaps { n, off, other_off } => {
                o.push(14.0);
                let (Some(off), Some(other_off)) = (triple_to_h(*off), triple_to_h(*other_off))
                else {
                    o.push(NULL_DEREF);
                    continue;
                };
                if !all_ok {
                    o.push(NULL_DEREF);
                    continue;
                }
                let r = geom.have_tangent_gaps(&k, &mut two, *n, off, other_off);
                let gaps = r.map_err(Fail::Solid)?;
                o.push(0.0);
                o.push(flag(gaps));
            }
            OpIn::EdgeuseOffset {
                orig,
                other_orig,
                off,
                other_off,
            } => {
                o.push(15.0);
                let (orig, other_orig, off, other_off) = (
                    triple_to_h(*orig),
                    triple_to_h(*other_orig),
                    triple_to_h(*off),
                    triple_to_h(*other_off),
                );
                let Some(orig) = orig else {
                    o.push(NULL_DEREF);
                    continue;
                };
                if !all_ok {
                    o.push(NULL_DEREF);
                    continue;
                }
                if i.dist.abs() > MAX_EDGEUSE_OFFSET_DISTANCE {
                    // the shim stops the script here (HANG-27)
                    return Err(Fail::Solid(SolidError::Unported {
                        upstream: "EdgeuseOffset beyond the HANG-27 distance bound (SmCrvOnSurf::ApproximateCurve of the rail)",
                        unit: HANG_27,
                    }));
                }
                if exec.extend_convex_edges() {
                    // a tangent or lamina edge goes to ExtendTrimEdges, which dereferences
                    // the offset edge use (null is refused) and its UV trim curves
                    let orig_edge = two.edgeuse_edge(orig)?.ok_or_else(|| invalid("edge"))?;
                    let lamina = two.edge_is_lamina(orig_edge)?;
                    let tangent =
                        two.edgeuse_is_tangent_sector(&k, orig, geom.tangency_tol_degrees())?;
                    if tangent || lamina {
                        let mut bad = off.is_none() || two.edgeuse_uv_trim_curve(orig)?.is_none();
                        if !bad && !lamina {
                            bad = match two.edgeuse_radial(orig)? {
                                Some(rad) => two.edgeuse_uv_trim_curve(rad)?.is_none(),
                                None => true,
                            };
                        }
                        if bad {
                            o.push(NULL_DEREF);
                            continue;
                        }
                        // ExtendTrimEdges' LiftCurve (lifts_iso_on_inexact_face)
                        if let Some(off) = off {
                            if two.lifts_iso_on_inexact_face(orig, off)? {
                                return Err(Fail::Solid(SolidError::Unported {
                                    upstream: "SmSurface::LiftCurve iso curve of a subclass (face surface imported as its B-spline part)",
                                    unit: ANALYTIC_EXPORT,
                                }));
                            }
                        }
                    }
                }
                let r = geom.edgeuse_offset(
                    &k, &mut two, &mut exec, orig, other_orig, off, other_off, mark,
                );
                if let Some(out) = status(r, &mut o)? {
                    // the out-parameters: only after success
                    o.push(flag(out.surface.is_some()));
                    if let Some(s) = out.surface {
                        put_srf(&k, &mut o, ClassGeometry::surface(&two, s));
                    }
                    put_hcurves(&mut o, &two, &out.trimming_curves)?;
                    put_orients(&mut o, &out.trim_orientations);
                    o.push(out.self_intersections.curves_3d.len() as f64);
                }
                o.push(flag(off.is_some())); // the edges' states: after every status
                if let Some(off) = off {
                    let e = two.edgeuse_edge(off)?.ok_or_else(|| invalid("edge"))?;
                    two.put_edge_state(&mut o, e)?;
                }
                o.push(flag(other_off.is_some()));
                if let Some(oo) = other_off {
                    let e = two.edgeuse_edge(oo)?.ok_or_else(|| invalid("edge"))?;
                    two.put_edge_state(&mut o, e)?;
                }
            }
            OpIn::EdgeState { e } => {
                o.push(17.0);
                let Some(e) = triple_to_h(*e) else {
                    o.push(NULL_DEREF);
                    continue;
                };
                o.push(0.0);
                two.put_edge_state(&mut o, e)?;
            }
            OpIn::SetEdgeTol { e, tol } => {
                o.push(18.0);
                let Some(e) = triple_to_h(*e) else {
                    o.push(NULL_DEREF);
                    continue;
                };
                ed::set_tolerance(two.brep_m(e)?, eid(e), *tol, false, false)?;
                o.push(0.0);
                o.push(two.edge_tolerance(e)?);
            }
        }
    }
    Ok(o)
}

/// The Rust surface of a surface input (what the shim's `read_surface` builds).
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

fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    let p = match prepare(i) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    match run_rust_script(i, p) {
        Ok(o) => Ok(Output::new().values("state", o)),
        Err(Fail::Solid(e)) => match Output::unported(&e) {
            Some(o) => Ok(o),
            None => Err(harness(format!("rust: {e}"))),
        },
        Err(Fail::Harness(h)) => Err(h),
    }
}

// ---- generation -----------------------------------------------------------------------------

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
    let mut u = vec![a; p + 1];
    let mut t: Vec<f64> = (0..inner)
        .map(|_| rng.uniform(a + 1e-3, b - 1e-3))
        .collect();
    t.sort_by(f64::total_cmp);
    u.extend(t);
    u.extend(vec![b; p + 1]);
    u
}

fn gen_curve(rng: &mut Rng, dim: u64, scale: f64) -> CrvIn {
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(4) as usize;
    let cpts = (0..=n)
        .map(|_| {
            let q = rng.point3(-scale, scale);
            [q[0], q[1], if dim == 2 { NOZ } else { q[2] }, NOW]
        })
        .collect();
    let (a, b) = if rng.chance(0.5) {
        (0.0, 1.0)
    } else {
        (rng.uniform(-2.0, 0.0), rng.uniform(1.0, 3.0))
    };
    CrvIn {
        dim,
        cpts,
        p,
        u: clamped_knots(rng, p, n, a, b),
    }
}

fn gen_surface(rng: &mut Rng) -> SrfIn {
    if rng.chance(0.3) {
        let o = rng.point3(-2.0, 2.0);
        let n = [
            rng.uniform(-1.0, 1.0),
            rng.uniform(-1.0, 1.0),
            rng.uniform(0.2, 1.0),
        ];
        return SrfIn::Plane { o, n };
    }
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let n = p + rng.below(3) as usize;
    let m = q + rng.below(3) as usize;
    let net = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    [
                        i as f64 + rng.uniform(-0.2, 0.2),
                        j as f64 + rng.uniform(-0.2, 0.2),
                        rng.uniform(-0.5, 0.5),
                        NOW,
                    ]
                })
                .collect()
        })
        .collect();
    SrfIn::Bspline(BssIn {
        oob: false,
        form: 0,
        net,
        p,
        q,
        u: clamped_knots(rng, p, n, 0.0, 1.0),
        v: clamped_knots(rng, q, m, 0.0, 1.0),
    })
}

fn gen_brep(rng: &mut Rng) -> BrepIn {
    match rng.below(20) {
        0 => BrepIn::Network,
        1 => {
            let curve = gen_curve(rng, 3, 2.0);
            let (a, b) = (
                curve.u.first().copied().unwrap_or(0.0),
                curve.u.last().copied().unwrap_or(1.0),
            );
            BrepIn::Wire {
                curve,
                t0: a,
                t1: b,
            }
        }
        2 => BrepIn::Face {
            surface: SrfIn::Plane {
                o: [0.0, 0.0, 0.0],
                n: [0.0, 0.0, 1.0],
            },
            dom: [0.0, 0.0, 1.0, 1.0],
        },
        _ => BrepIn::File {
            file: (*rng.choose(&FILES).unwrap_or(&"cone.smb")).to_string(),
            rebuild: false,
        },
    }
}

fn gen_sel(rng: &mut Rng) -> i64 {
    rng.below(64) as i64
}

fn gen_h(rng: &mut Rng, has_b: bool, kinds: &[u8], null: f64) -> Hin {
    if rng.chance(null) {
        return [-1, K_NULL, -1];
    }
    let b = i64::from(has_b && rng.chance(0.5));
    let kind = *rng.choose(kinds).unwrap_or(&K_EDGE);
    [b, i64::from(kind), gen_sel(rng)]
}

fn gen_eu(rng: &mut Rng, b: i64, null: f64) -> Hin {
    if rng.chance(null) {
        return [-1, K_NULL, -1];
    }
    [b, i64::from(K_EDGEUSE), gen_sel(rng)]
}

fn gen_curve_h(rng: &mut Rng, null: f64) -> Hin {
    if rng.chance(null) {
        return [-1, K_NULL, -1];
    }
    [2, i64::from(K_CURVE), gen_sel(rng)]
}

/// The B-rep the "offset" edge uses live in.
fn off_b(has_b: bool) -> i64 {
    i64::from(has_b)
}

#[allow(clippy::too_many_lines)]
fn gen_op(rng: &mut Rng, has_b: bool) -> OpIn {
    let ob = off_b(has_b);
    match rng.below(19) {
        0 => {
            let dim = if rng.chance(0.5) { 2 } else { 3 };
            let scale = *rng.choose(&[1.0, 10.0, 0.01]).unwrap_or(&1.0);
            OpIn::AddCurve {
                curve: gen_curve(rng, dim, scale),
            }
        }
        1 | 2 => {
            let d = if rng.chance(0.3) {
                [0.0, 0.0, 0.0]
            } else {
                let s = *rng.choose(&[0.1, 0.5, 1.0, 0.01]).unwrap_or(&0.5);
                rng.point3(-s, s)
            };
            OpIn::AddCurveFrom {
                src: gen_h(rng, has_b, &[K_EDGE, K_EDGE, K_EDGEUSE], 0.0),
                d,
                scale: *rng.choose(&[1.0, 1.0, 1.1, 0.9, 2.0]).unwrap_or(&1.0),
            }
        }
        3 => OpIn::AddSurface {
            surface: gen_surface(rng),
        },
        4 => OpIn::SetEdgeCurve {
            e: gen_h(rng, has_b, &[K_EDGE], 0.0),
            c: gen_curve_h(rng, 0.0),
        },
        5 => OpIn::SetEuUv {
            eu: gen_h(rng, has_b, &[K_EDGEUSE], 0.0),
            c: gen_curve_h(rng, 0.2),
            max_dist: *rng.choose(&[0.0, 1e-5, 1e-3]).unwrap_or(&0.0),
        },
        6 => OpIn::Mark {
            eu: gen_eu(rng, ob, 0.0),
        },
        7 => OpIn::Getters,
        8 => OpIn::FaceuseOffset {
            fu: gen_h(rng, has_b, &[K_FACEUSE], 0.05),
            skip: rng.chance(0.5),
        },
        9 => {
            let n = match rng.below(6) {
                0 => 0,
                1 | 2 => 1,
                3 => 2,
                _ => 3,
            };
            OpIn::VertexOffset {
                v: gen_h(rng, has_b, &[K_VERTEX], 0.05),
                edges: (0..n).map(|_| gen_h(rng, has_b, &[K_EDGE], 0.05)).collect(),
            }
        }
        10 => OpIn::UpdateFaceDomain {
            f: gen_h(rng, has_b, &[K_FACE], 0.05),
            s: gen_h(rng, has_b, &[K_SURFACE, K_FACE_SURFACE], 0.05),
        },
        11 => OpIn::ReplaceEdgeCurves {
            eu: gen_eu(rng, ob, 0.05),
            c3: gen_curve_h(rng, 0.3),
            uv: gen_curve_h(rng, 0.3),
            orig: gen_eu(rng, 0, 0.5),
        },
        12 => OpIn::TrimExtendedEdge {
            base: gen_eu(rng, 0, 0.05),
            other_base: gen_eu(rng, 0, 0.3),
            off: gen_eu(rng, ob, 0.5),
            other_off: gen_eu(rng, ob, 0.5),
            c3: if rng.chance(0.5) {
                gen_curve_h(rng, 0.05)
            } else {
                gen_h(rng, has_b, &[K_EDGE_CURVE], 0.05)
            },
            uv_base: gen_curve_h(rng, 0.3),
            uv_other: gen_curve_h(rng, 0.5),
        },
        13 => OpIn::ExtendTrimEdges {
            eu: gen_eu(rng, 0, 0.05),
            other: gen_eu(rng, 0, 0.3),
            off: gen_eu(rng, ob, 0.05),
            other_off: gen_eu(rng, ob, 0.3),
        },
        14 => OpIn::MateEdges {
            eu: gen_eu(rng, 0, 0.2),
            other: gen_eu(rng, 0, 0.2),
            off: gen_eu(rng, ob, 0.2),
            other_off: gen_eu(rng, ob, 0.2),
        },
        15 => OpIn::HaveTangentGaps {
            n: 1 + rng.below(12),
            off: gen_eu(rng, ob, 0.1),
            other_off: gen_eu(rng, ob, 0.1),
        },
        16 => OpIn::EdgeuseOffset {
            orig: gen_eu(rng, 0, 0.05),
            other_orig: gen_eu(rng, 0, 0.2),
            off: gen_eu(rng, ob, 0.2),
            other_off: gen_eu(rng, ob, 0.2),
        },
        17 => OpIn::LocalFaceuseOffset {
            fu: gen_h(rng, has_b, &[K_FACEUSE], 0.05),
            skip: rng.chance(0.5),
        },
        _ => {
            if rng.chance(0.5) {
                OpIn::EdgeState {
                    e: gen_h(rng, has_b, &[K_EDGE], 0.05),
                }
            } else {
                OpIn::SetEdgeTol {
                    e: gen_h(rng, has_b, &[K_EDGE], 0.05),
                    tol: *rng.choose(&[1e-6, 1e-4, 1e-2, 0.0]).unwrap_or(&1e-4),
                }
            }
        }
    }
}

fn generate(rng: &mut Rng) -> Option<Value> {
    let brep_a = gen_brep(rng);
    let has_b = rng.chance(0.8);
    let brep_b = if has_b {
        Some(if rng.chance(0.6) {
            brep_a.clone()
        } else {
            gen_brep(rng)
        })
    } else {
        None
    };
    let dist = *rng
        .choose(&[0.1, 0.5, 1.0, -0.25, 2.0, 1e-3, 100.0, 0.05])
        .unwrap_or(&0.5);
    let approx = *rng.choose(&[1e-5, 1e-5, 1e-4, 1e-3, 1e-6]).unwrap_or(&1e-5);
    let ob = off_b(has_b);
    let n_pairs = rng.below(12) as usize;
    let pairs = (0..n_pairs)
        .map(|j| {
            let s = j as i64 + rng.below(3) as i64 * 7;
            [[0, i64::from(K_EDGEUSE), s], [ob, i64::from(K_EDGEUSE), s]]
        })
        .collect();
    let local = (0..rng.below(3))
        .map(|_| {
            [
                gen_h(rng, has_b, &[K_FACE], 0.1),
                gen_h(rng, has_b, &[K_SURFACE], 0.1),
            ]
        })
        .collect();
    let shell = (0..rng.below(3))
        .map(|_| gen_h(rng, false, &[K_FACE], 0.1))
        .collect();
    let n = 1 + rng.below(6) as usize;
    let mut ops = Vec::new();
    // some store geometry first, so that the curve handles resolve
    if rng.chance(0.7) {
        ops.push(OpIn::AddCurve {
            curve: gen_curve(rng, 2, 1.0),
        });
    }
    if rng.chance(0.7) {
        ops.push(gen_op(rng, has_b));
        if !matches!(ops.last(), Some(OpIn::AddCurveFrom { .. })) {
            ops.push(OpIn::AddCurveFrom {
                src: gen_h(rng, has_b, &[K_EDGE], 0.0),
                d: rng.point3(-0.5, 0.5),
                scale: 1.0,
            });
        }
    }
    for _ in 0..n {
        ops.push(gen_op(rng, has_b));
    }
    serde_json::to_value(Inputs {
        brep_a,
        brep_b,
        dist,
        approx,
        analytics: rng.chance(0.5),
        local,
        inset: rng.chance(0.3),
        extend: rng.chance(0.3),
        shell,
        pairs,
        ops,
    })
    .ok()
}

// ---- the runner ---------------------------------------------------------------------------

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "offset.offset_geometry_creation"
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        let mut cases = default_cases(self, &ctx)?;
        for (name, inputs) in edge_cases() {
            cases.push(Case {
                unit: self.id().into(),
                name,
                source: CaseSource::Inline,
                inputs,
            });
        }
        let zero = zero_header_cases(self.id(), &cases)?;
        cases.extend(zero);
        Ok(cases)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        run_cpp(&i)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        run_rust(&i)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        generate(rng)
    }
}

fn file(name: &str) -> BrepIn {
    BrepIn::File {
        file: name.to_string(),
        rebuild: false,
    }
}

const fn h(b: i64, kind: u8, i: i64) -> Hin {
    [b, kind as i64, i]
}

const NULL_H: Hin = [-1, K_NULL, -1];

/// Cases built in code: each op on real solid B-reps with explicit geometry, at unit,
/// tiny and huge scale, plus the guard refusals on the wire network (the run header at
/// zero is [`zero_header_cases`]).
#[allow(clippy::too_many_lines)]
fn edge_cases() -> Vec<(String, Value)> {
    let mut out = Vec::new();
    let mut add = |name: &str, i: Inputs| {
        if let Ok(v) = serde_json::to_value(i) {
            out.push((name.to_string(), v));
        }
    };
    let base = |a: &str, b: Option<&str>, dist: f64| Inputs {
        brep_a: file(a),
        brep_b: b.map(file),
        dist,
        approx: 1e-5,
        analytics: true,
        local: Vec::new(),
        inset: false,
        extend: false,
        shell: Vec::new(),
        pairs: Vec::new(),
        ops: Vec::new(),
    };
    let pairs = |n: i64, ob: i64| -> Vec<[Hin; 2]> {
        (0..n)
            .map(|j| [h(0, K_EDGEUSE, j), h(ob, K_EDGEUSE, j)])
            .collect()
    };

    // Concrete input records must reach the actual virtual offset, not the
    // B-spline stand-in. Visit both orientations of every analytic fixture face;
    // skip only the caller-selected self-intersection stage, not geometry creation.
    for (name, f, faceuses) in [
        ("cone", "cone.smb", 6),
        // This legacy file stores its cylindrical wall as an SmCone with zero
        // semi-angle, not as an SmCylinder; preserve its actual concrete kind.
        ("zero_angle_cone", "Solids/cylinder.smb", 6),
        ("sphere", "sphere.smb", 2),
        ("torus_revolution", "torus.smb", 2),
        ("extrusion", "surf_extrude.smb", 2),
        ("revolution", "surf_rev.smb", 2),
    ] {
        for (sign, distance) in [("negative", -0.1), ("zero", 0.0), ("positive", 0.1)] {
            let mut i = base(f, Some(f), distance);
            i.ops = (0..faceuses)
                .map(|faceuse| OpIn::FaceuseOffset {
                    fu: h(0, K_FACEUSE, faceuse),
                    skip: true,
                })
                .collect();
            add(&format!("analytic_import_{name}_{sign}"), i);
        }
    }
    for (name, fixture) in [
        (
            "native_cylinder",
            BrepIn::Cylinder {
                radius: 2.0,
                height: 3.0,
            },
        ),
        (
            "native_torus",
            BrepIn::Torus {
                major: 3.0,
                minor: 1.0,
            },
        ),
    ] {
        for (sign, distance) in [("negative", -0.1), ("zero", 0.0), ("positive", 0.1)] {
            let mut i = base("cone.smb", None, distance);
            i.brep_a = fixture.clone();
            i.brep_b = Some(fixture.clone());
            i.ops = (0..2)
                .map(|faceuse| OpIn::FaceuseOffset {
                    fu: h(0, K_FACEUSE, faceuse),
                    skip: true,
                })
                .collect();
            add(&format!("analytic_import_{name}_{sign}"), i);
        }
    }

    // getters and the fixture ops
    let mut i = base("cone.smb", Some("cone.smb"), 0.5);
    i.pairs = pairs(8, 1);
    i.ops = vec![
        OpIn::Getters,
        OpIn::EdgeState { e: h(1, K_EDGE, 0) },
        OpIn::AddCurveFrom {
            src: h(1, K_EDGE, 0),
            d: [0.0, 0.0, 0.5],
            scale: 1.0,
        },
        OpIn::SetEdgeCurve {
            e: h(1, K_EDGE, 0),
            c: h(2, K_CURVE, 0),
        },
        OpIn::AddCurveFrom {
            src: h(0, K_EDGEUSE, 0),
            d: [0.1, 0.0, 0.0],
            scale: 1.0,
        },
        OpIn::SetEuUv {
            eu: h(1, K_EDGEUSE, 0),
            c: h(2, K_CURVE, 1),
            max_dist: 1e-4,
        },
        OpIn::SetEdgeTol {
            e: h(1, K_EDGE, 1),
            tol: 1e-3,
        },
        OpIn::Mark {
            eu: h(1, K_EDGEUSE, 0),
        },
        OpIn::EdgeState { e: h(1, K_EDGE, 0) },
    ];
    add("cone_fixture_ops", i);

    // LocalFaceuseOffset on listed faces with store surfaces (the local creation's own
    // surfaces): with analytics the listed surface is reparameterised, copied and consumed,
    // so a second op on the same face is refused; without analytics it is returned as is;
    // an unlisted face is SM_ERR.
    for (analytics, name) in [
        (true, "local_offset_analytics"),
        (false, "local_offset_plain"),
    ] {
        let mut i = base("cone.smb", None, 0.5);
        i.analytics = analytics;
        i.local = (0..3)
            .map(|f| [h(0, K_FACE, f), h(2, K_SURFACE, f)])
            .collect();
        i.ops = (0..3)
            .map(|f| OpIn::AddSurface {
                surface: SrfIn::Plane {
                    o: [0.0, 0.0, 0.25 * f64::from(f)],
                    n: [0.0, 0.0, 1.0],
                },
            })
            .collect();
        i.ops.extend([
            OpIn::LocalFaceuseOffset {
                fu: h(0, K_FACEUSE, 0),
                skip: false,
            },
            OpIn::LocalFaceuseOffset {
                fu: h(0, K_FACEUSE, 0),
                skip: true,
            },
            OpIn::LocalFaceuseOffset {
                fu: h(0, K_FACEUSE, 1),
                skip: false,
            },
            OpIn::LocalFaceuseOffset {
                fu: h(0, K_FACEUSE, 5),
                skip: false,
            },
        ]);
        add(name, i);
    }
    // a listed face whose store surface is added only after the first local op
    let mut i = base("plane_square.smb", None, 0.1);
    i.local = vec![[h(0, K_FACE, 0), h(2, K_SURFACE, 0)]];
    i.ops = vec![
        OpIn::LocalFaceuseOffset {
            fu: h(0, K_FACEUSE, 0),
            skip: false,
        },
        OpIn::AddSurface {
            surface: SrfIn::Plane {
                o: [0.0, 0.0, 0.0],
                n: [0.0, 0.0, 1.0],
            },
        },
        OpIn::LocalFaceuseOffset {
            fu: h(0, K_FACEUSE, 0),
            skip: false,
        },
    ];
    add("local_offset_late_surface", i);

    // ReplaceEdgeCurves: same ends (delete flags), reversed copy, UV only, null UV with
    // a reversed curve (refused), a moved copy (vertex moves: deferred)
    for (name, f) in [
        ("cone", "cone.smb"),
        ("torus", "torus.smb"),
        ("box", "box_with_large_fillet.smb"),
    ] {
        let mut i = base(f, Some(f), 0.25);
        i.pairs = pairs(4, 1);
        i.ops = vec![
            OpIn::AddCurveFrom {
                src: h(1, K_EDGE, 0),
                d: [0.0; 3],
                scale: 1.0,
            },
            OpIn::AddCurve {
                curve: CrvIn {
                    dim: 2,
                    cpts: vec![[0.0, 0.0, NOZ, NOW], [1.0, 1.0, NOZ, NOW]],
                    p: 1,
                    u: vec![0.0, 0.0, 1.0, 1.0],
                },
            },
            OpIn::ReplaceEdgeCurves {
                eu: h(1, K_EDGEUSE, 0),
                c3: h(2, K_CURVE, 0),
                uv: h(2, K_CURVE, 1),
                orig: NULL_H,
            },
            OpIn::ReplaceEdgeCurves {
                eu: h(1, K_EDGEUSE, 0),
                c3: NULL_H,
                uv: h(2, K_CURVE, 1),
                orig: NULL_H,
            },
            OpIn::ReplaceEdgeCurves {
                eu: h(1, K_EDGEUSE, 1),
                c3: NULL_H,
                uv: NULL_H,
                orig: h(0, K_EDGEUSE, 1),
            },
            OpIn::AddCurveFrom {
                src: h(1, K_EDGE, 1),
                d: [0.3, 0.0, 0.0],
                scale: 1.0,
            },
            OpIn::ReplaceEdgeCurves {
                eu: h(1, K_EDGEUSE, 2),
                c3: h(2, K_CURVE, 2),
                uv: NULL_H,
                orig: h(0, K_EDGEUSE, 2),
            },
            OpIn::ReplaceEdgeCurves {
                eu: h(1, K_EDGEUSE, 2),
                c3: h(2, K_CURVE, 2),
                uv: h(2, K_CURVE, 1),
                orig: h(0, K_EDGEUSE, 2),
            },
        ];
        add(&format!("{name}_replace_edge_curves"), i);
    }

    // VertexOffset: one, two and three bounding edges of real solids
    for (name, f, dist) in [
        ("cone", "cone.smb", 0.5),
        ("box", "box_with_large_fillet.smb", 0.25),
        ("sphere", "sphere.smb", 1.0),
        ("torus", "torus.smb", 0.1),
        ("vase", "vase_rev.smb", 2.0),
    ] {
        let mut i = base(f, Some(f), dist);
        i.ops = vec![
            OpIn::VertexOffset {
                v: h(0, K_VERTEX, 0),
                edges: vec![h(1, K_EDGE, 0)],
            },
            OpIn::VertexOffset {
                v: h(0, K_VERTEX, 0),
                edges: vec![h(1, K_EDGE, 0), h(1, K_EDGE, 1)],
            },
            OpIn::VertexOffset {
                v: h(0, K_VERTEX, 0),
                edges: vec![h(1, K_EDGE, 0), h(1, K_EDGE, 0)],
            },
            OpIn::VertexOffset {
                v: h(0, K_VERTEX, 1),
                edges: vec![h(1, K_EDGE, 0), h(1, K_EDGE, 1), h(1, K_EDGE, 2)],
            },
            OpIn::VertexOffset {
                v: h(0, K_VERTEX, 1),
                edges: vec![
                    h(1, K_EDGE, 0),
                    h(1, K_EDGE, 1),
                    h(1, K_EDGE, 2),
                    h(1, K_EDGE, 3),
                ],
            },
            OpIn::VertexOffset {
                v: NULL_H,
                edges: vec![h(1, K_EDGE, 0)],
            },
            OpIn::VertexOffset {
                v: h(0, K_VERTEX, 0),
                edges: Vec::new(),
            },
        ];
        add(&format!("{name}_vertex_offset"), i);
    }

    // VertexOffset with three arcs of one plane-sized sphere: replace three edges of the
    // box by circle arcs through the offset corner
    {
        let mut i = base(
            "box_with_large_fillet.smb",
            Some("box_with_large_fillet.smb"),
            0.5,
        );
        let arc = |a: [f64; 3], m: [f64; 3], b: [f64; 3]| CrvIn {
            dim: 3,
            cpts: vec![
                [a[0], a[1], a[2], NOW],
                [m[0], m[1], m[2], NOW],
                [b[0], b[1], b[2], NOW],
            ],
            p: 2,
            u: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        };
        let r = 0.5;
        let w = std::f64::consts::FRAC_1_SQRT_2;
        // quadratic rational arcs of radius r about the origin in the three coordinate planes
        let rat = |a: [f64; 3], m: [f64; 3], b: [f64; 3]| {
            let mut c = arc(a, m, b);
            c.cpts = vec![
                [a[0], a[1], a[2], 1.0],
                [m[0] * w, m[1] * w, m[2] * w, w],
                [b[0], b[1], b[2], 1.0],
            ];
            c
        };
        i.ops = vec![
            OpIn::AddCurve {
                curve: rat([r, 0.0, 0.0], [r, r, 0.0], [0.0, r, 0.0]),
            },
            OpIn::AddCurve {
                curve: rat([0.0, r, 0.0], [0.0, r, r], [0.0, 0.0, r]),
            },
            OpIn::AddCurve {
                curve: rat([0.0, 0.0, r], [r, 0.0, r], [r, 0.0, 0.0]),
            },
            OpIn::SetEdgeCurve {
                e: h(1, K_EDGE, 0),
                c: h(2, K_CURVE, 0),
            },
            OpIn::SetEdgeCurve {
                e: h(1, K_EDGE, 1),
                c: h(2, K_CURVE, 1),
            },
            OpIn::SetEdgeCurve {
                e: h(1, K_EDGE, 2),
                c: h(2, K_CURVE, 2),
            },
            OpIn::VertexOffset {
                v: h(0, K_VERTEX, 0),
                edges: vec![h(1, K_EDGE, 0), h(1, K_EDGE, 1), h(1, K_EDGE, 2)],
            },
            // a line and two arcs: not all arcs
            OpIn::AddCurve {
                curve: arc([0.0, 0.0, 0.0], [0.5, 0.5, 0.0], [1.0, 1.0, 0.0]),
            },
            OpIn::SetEdgeCurve {
                e: h(1, K_EDGE, 2),
                c: h(2, K_CURVE, 3),
            },
            OpIn::VertexOffset {
                v: h(0, K_VERTEX, 0),
                edges: vec![h(1, K_EDGE, 0), h(1, K_EDGE, 1), h(1, K_EDGE, 2)],
            },
        ];
        add("box_vertex_offset_three_arcs", i);
    }

    // EdgeuseOffset on real solids: convex / tangent sectors, nulls, shell faces, inset
    for (name, f, dist) in [
        ("cone", "cone.smb", 0.5),
        ("box", "box_with_large_fillet.smb", 0.25),
        ("torus", "torus.smb", 0.1),
        ("endcap", "Endcap.smb", 0.5),
        ("mouse", "Mouse.smb", 1.0),
    ] {
        for (variant, inset, extend, shell, analytics) in [
            ("plain", false, false, false, true),
            ("inset_shell", true, false, true, false),
            ("extend", false, true, false, true),
            ("shell", false, false, true, false),
        ] {
            let mut i = base(f, Some(f), dist);
            i.inset = inset;
            i.extend = extend;
            i.analytics = analytics;
            if shell {
                i.shell = vec![h(0, K_FACE, 0)];
            }
            i.pairs = pairs(12, 1);
            i.ops = vec![
                OpIn::EdgeuseOffset {
                    orig: h(0, K_EDGEUSE, 0),
                    other_orig: h(0, K_EDGEUSE, 1),
                    off: h(1, K_EDGEUSE, 0),
                    other_off: h(1, K_EDGEUSE, 1),
                },
                OpIn::EdgeuseOffset {
                    orig: h(0, K_EDGEUSE, 2),
                    other_orig: NULL_H,
                    off: h(1, K_EDGEUSE, 2),
                    other_off: NULL_H,
                },
                OpIn::EdgeuseOffset {
                    orig: h(0, K_EDGEUSE, 3),
                    other_orig: h(0, K_EDGEUSE, 4),
                    off: NULL_H,
                    other_off: h(1, K_EDGEUSE, 4),
                },
                OpIn::EdgeuseOffset {
                    orig: h(0, K_EDGEUSE, 5),
                    other_orig: h(0, K_EDGEUSE, 5),
                    off: h(1, K_EDGEUSE, 5),
                    other_off: h(1, K_EDGEUSE, 5),
                },
                OpIn::HaveTangentGaps {
                    n: 10,
                    off: h(1, K_EDGEUSE, 0),
                    other_off: h(1, K_EDGEUSE, 1),
                },
            ];
            add(&format!("{name}_edgeuse_offset_{variant}"), i);
        }
    }

    // EdgeuseOffset with crafted rails: a torus (arcs), a self-intersecting torus, a
    // cylinder (lines) and rails within ten tolerances
    {
        let circle = |r: f64, z: f64| {
            let w = std::f64::consts::FRAC_1_SQRT_2;
            CrvIn {
                dim: 3,
                cpts: vec![
                    [r, 0.0, z, 1.0],
                    [r * w, r * w, z * w, w],
                    [0.0, r, z, 1.0],
                    [-r * w, r * w, z * w, w],
                    [-r, 0.0, z, 1.0],
                ],
                p: 2,
                u: vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
            }
        };
        let line = |a: [f64; 3], b: [f64; 3]| CrvIn {
            dim: 3,
            cpts: vec![[a[0], a[1], a[2], NOW], [b[0], b[1], b[2], NOW]],
            p: 1,
            u: vec![0.0, 0.0, 1.0, 1.0],
        };
        for (name, dist, c0, c1, c2) in [
            (
                "torus",
                0.5,
                circle(2.0, 0.0),
                circle(2.5, 0.0),
                circle(1.5, 0.0),
            ),
            (
                "torus_self_intersecting",
                3.0,
                circle(2.0, 0.0),
                circle(5.0, 0.0),
                circle(2.0, 3.0),
            ),
            (
                "torus_vertical",
                1.0,
                circle(2.0, 0.0),
                circle(2.0, 1.0),
                circle(2.0, -1.0),
            ),
            (
                "cylinder",
                0.5,
                line([0.0, 0.0, 0.0], [0.0, 0.0, 2.0]),
                line([0.5, 0.0, 0.0], [0.5, 0.0, 2.0]),
                line([0.0, 0.5, 0.0], [0.0, 0.5, 2.0]),
            ),
            (
                "cone",
                0.5,
                line([0.0, 0.0, 0.0], [0.0, 0.0, 2.0]),
                line([0.5, 0.0, 0.0], [0.5, 0.0, 2.0]),
                line([0.5, 0.0, 0.0], [0.0, 0.5, 2.0]),
            ),
            (
                "close_rails",
                1e-3,
                line([0.0, 0.0, 0.0], [0.0, 0.0, 2.0]),
                line([1e-5, 0.0, 0.0], [1e-5, 0.0, 2.0]),
                line([0.0, 1e-5, 0.0], [0.0, 1e-5, 2.0]),
            ),
            (
                "mismatched_intervals",
                0.5,
                line([0.0, 0.0, 0.0], [0.0, 0.0, 2.0]),
                line([0.5, 0.0, 0.0], [0.5, 0.0, 2.0]),
                CrvIn {
                    dim: 3,
                    cpts: vec![[0.0, 0.5, 0.0, NOW], [0.0, 0.5, 2.0, NOW]],
                    p: 1,
                    u: vec![0.0, 0.0, 3.0, 3.0],
                },
            ),
        ] {
            for f in ["box_with_large_fillet.smb", "cone.smb"] {
                let mut i = base(f, Some(f), dist);
                i.pairs = pairs(6, 1);
                i.ops = vec![
                    OpIn::AddCurve { curve: c0.clone() },
                    OpIn::AddCurve { curve: c1.clone() },
                    OpIn::AddCurve { curve: c2.clone() },
                    OpIn::SetEdgeCurve {
                        e: h(0, K_EDGE, 0),
                        c: h(2, K_CURVE, 0),
                    },
                    OpIn::SetEdgeCurve {
                        e: h(1, K_EDGE, 0),
                        c: h(2, K_CURVE, 1),
                    },
                    OpIn::SetEdgeCurve {
                        e: h(1, K_EDGE, 1),
                        c: h(2, K_CURVE, 2),
                    },
                    OpIn::EdgeuseOffset {
                        orig: h(0, K_EDGEUSE, 0),
                        other_orig: h(0, K_EDGEUSE, 1),
                        off: h(1, K_EDGEUSE, 0),
                        other_off: h(1, K_EDGEUSE, 2),
                    },
                    OpIn::EdgeState { e: h(1, K_EDGE, 0) },
                    OpIn::EdgeState { e: h(1, K_EDGE, 1) },
                ];
                let short = f.split('.').next().unwrap_or("f");
                add(&format!("{short}_edgeuse_offset_rails_{name}"), i);
            }
        }
    }

    // TrimExtendedEdge: drop of the base vertex, marked neighbours, UV intersections
    for (name, f) in [
        ("cone", "cone.smb"),
        ("box", "box_with_large_fillet.smb"),
        ("torus", "torus.smb"),
        ("vase", "vase_rev.smb"),
    ] {
        let mut i = base(f, Some(f), 0.5);
        i.pairs = pairs(16, 1);
        i.ops = vec![
            OpIn::AddCurveFrom {
                src: h(0, K_EDGE, 0),
                d: [0.0, 0.0, 0.0],
                scale: 1.0,
            },
            OpIn::AddCurveFrom {
                src: h(0, K_EDGEUSE, 0),
                d: [0.0, 0.0, 0.0],
                scale: 1.0,
            },
            OpIn::AddCurveFrom {
                src: h(0, K_EDGEUSE, 1),
                d: [0.0, 0.0, 0.0],
                scale: 1.0,
            },
            OpIn::TrimExtendedEdge {
                base: h(0, K_EDGEUSE, 0),
                other_base: h(0, K_EDGEUSE, 1),
                off: h(1, K_EDGEUSE, 0),
                other_off: h(1, K_EDGEUSE, 1),
                c3: h(2, K_CURVE, 0),
                uv_base: h(2, K_CURVE, 1),
                uv_other: h(2, K_CURVE, 2),
            },
            OpIn::TrimExtendedEdge {
                base: h(0, K_EDGEUSE, 0),
                other_base: NULL_H,
                off: NULL_H,
                other_off: NULL_H,
                c3: h(0, K_EDGE_CURVE, 0),
                uv_base: NULL_H,
                uv_other: NULL_H,
            },
            OpIn::Mark {
                eu: h(0, K_EDGEUSE, 2),
            },
            OpIn::Mark {
                eu: h(0, K_EDGEUSE, 3),
            },
            OpIn::Mark {
                eu: h(0, K_EDGEUSE, 4),
            },
            OpIn::Mark {
                eu: h(0, K_EDGEUSE, 5),
            },
            OpIn::TrimExtendedEdge {
                base: h(0, K_EDGEUSE, 0),
                other_base: h(0, K_EDGEUSE, 1),
                off: h(1, K_EDGEUSE, 0),
                other_off: h(1, K_EDGEUSE, 1),
                c3: h(2, K_CURVE, 0),
                uv_base: h(2, K_CURVE, 1),
                uv_other: h(2, K_CURVE, 2),
            },
            OpIn::AddCurveFrom {
                src: h(0, K_EDGE, 0),
                d: [0.2, 0.1, 0.0],
                scale: 1.0,
            },
            OpIn::TrimExtendedEdge {
                base: h(0, K_EDGEUSE, 0),
                other_base: h(0, K_EDGEUSE, 1),
                off: h(1, K_EDGEUSE, 0),
                other_off: h(1, K_EDGEUSE, 1),
                c3: h(2, K_CURVE, 3),
                uv_base: h(2, K_CURVE, 1),
                uv_other: NULL_H,
            },
        ];
        add(&format!("{name}_trim_extended_edge"), i);
    }

    // MateEdges nulls, ExtendTrimEdges and the local creation
    {
        let mut i = base("cone.smb", Some("cone.smb"), 0.5);
        i.pairs = pairs(8, 1);
        i.local = vec![
            [h(0, K_FACE, 0), h(2, K_SURFACE, 0)],
            [h(0, K_FACE, 1), NULL_H],
        ];
        i.ops = vec![
            OpIn::AddSurface {
                surface: SrfIn::Plane {
                    o: [0.0, 0.0, 1.0],
                    n: [0.0, 0.0, 1.0],
                },
            },
            OpIn::MateEdges {
                eu: NULL_H,
                other: h(0, K_EDGEUSE, 1),
                off: h(1, K_EDGEUSE, 0),
                other_off: h(1, K_EDGEUSE, 1),
            },
            OpIn::MateEdges {
                eu: h(0, K_EDGEUSE, 0),
                other: h(0, K_EDGEUSE, 1),
                off: h(1, K_EDGEUSE, 0),
                other_off: NULL_H,
            },
            OpIn::MateEdges {
                eu: h(0, K_EDGEUSE, 0),
                other: h(0, K_EDGEUSE, 1),
                off: h(1, K_EDGEUSE, 0),
                other_off: h(1, K_EDGEUSE, 1),
            },
            OpIn::ExtendTrimEdges {
                eu: h(0, K_EDGEUSE, 0),
                other: h(0, K_EDGEUSE, 1),
                off: h(1, K_EDGEUSE, 0),
                other_off: h(1, K_EDGEUSE, 1),
            },
            OpIn::LocalFaceuseOffset {
                fu: h(0, K_FACEUSE, 0),
                skip: false,
            },
            OpIn::LocalFaceuseOffset {
                fu: h(0, K_FACEUSE, 1),
                skip: true,
            },
            OpIn::LocalFaceuseOffset {
                fu: h(0, K_FACEUSE, 2),
                skip: true,
            },
            OpIn::FaceuseOffset {
                fu: h(0, K_FACEUSE, 0),
                skip: true,
            },
            OpIn::UpdateFaceDomain {
                f: h(0, K_FACE, 0),
                s: h(2, K_SURFACE, 0),
            },
        ];
        add("cone_mate_extend_local", i);
    }

    // the guard refusals on non-solid B-reps
    for (name, a) in [
        ("network", BrepIn::Network),
        (
            "face",
            BrepIn::Face {
                surface: SrfIn::Plane {
                    o: [0.0, 0.0, 0.0],
                    n: [0.0, 0.0, 1.0],
                },
                dom: [0.0, 0.0, 1.0, 1.0],
            },
        ),
    ] {
        let mut i = base("cone.smb", None, 0.5);
        i.brep_a = a;
        i.ops = vec![
            OpIn::Getters,
            OpIn::VertexOffset {
                v: h(0, K_VERTEX, 0),
                edges: vec![h(0, K_EDGE, 0)],
            },
            OpIn::EdgeuseOffset {
                orig: h(0, K_EDGEUSE, 0),
                other_orig: h(0, K_EDGEUSE, 1),
                off: h(0, K_EDGEUSE, 2),
                other_off: h(0, K_EDGEUSE, 3),
            },
            OpIn::TrimExtendedEdge {
                base: h(0, K_EDGEUSE, 0),
                other_base: NULL_H,
                off: NULL_H,
                other_off: NULL_H,
                c3: h(0, K_EDGE_CURVE, 0),
                uv_base: NULL_H,
                uv_other: NULL_H,
            },
            OpIn::MateEdges {
                eu: h(0, K_EDGEUSE, 0),
                other: h(0, K_EDGEUSE, 1),
                off: h(0, K_EDGEUSE, 2),
                other_off: h(0, K_EDGEUSE, 3),
            },
            OpIn::EdgeState { e: h(0, K_EDGE, 0) },
        ];
        add(&format!("{name}_guards"), i);
    }

    // scale: tiny and huge offsets on the vase
    for (name, dist, approx) in [("tiny", 1e-4, 1e-8), ("huge", 1e3, 1e-2)] {
        let mut i = base("vase_rev.smb", Some("vase_rev.smb"), dist);
        i.approx = approx;
        i.pairs = pairs(8, 1);
        i.ops = vec![
            OpIn::VertexOffset {
                v: h(0, K_VERTEX, 0),
                edges: vec![h(1, K_EDGE, 0), h(1, K_EDGE, 1)],
            },
            OpIn::EdgeuseOffset {
                orig: h(0, K_EDGEUSE, 0),
                other_orig: h(0, K_EDGEUSE, 1),
                off: h(1, K_EDGEUSE, 0),
                other_off: h(1, K_EDGEUSE, 1),
            },
        ];
        add(&format!("vase_{name}_scale"), i);
    }

    // VertexOffset on topology without loops (it walks none, so the shim no longer
    // refuses it there): the wire network's vertices with two and three of their edges,
    // and with no edges; a single wire's end vertex with its own edge (refused: the
    // one-edge branch needs a face)
    for (name, v, edges) in [
        ("network_vertex_two_edges", 0, vec![0, 1]),
        ("network_vertex_three_edges", 1, vec![1, 2, 3]),
        ("network_vertex_no_edges", 2, Vec::new()),
    ] {
        let mut i = base("cone.smb", None, 0.5);
        i.brep_a = BrepIn::Network;
        i.ops = vec![OpIn::VertexOffset {
            v: h(0, K_VERTEX, v),
            edges: edges.into_iter().map(|e| h(0, K_EDGE, e)).collect(),
        }];
        add(name, i);
    }
    {
        let mut i = base("cone.smb", None, 0.5);
        i.brep_a = BrepIn::Wire {
            curve: CrvIn {
                dim: 3,
                cpts: vec![
                    [0.0, 0.0, 0.0, 1.0],
                    [1.0, 2.0, 0.0, 1.0],
                    [3.0, 1.0, 1.0, 1.0],
                ],
                p: 2,
                u: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            },
            t0: 0.0,
            t1: 1.0,
        };
        i.ops = vec![
            OpIn::VertexOffset {
                v: h(0, K_VERTEX, 0),
                edges: vec![h(0, K_EDGE, 0)],
            },
            OpIn::VertexOffset {
                v: h(0, K_VERTEX, 1),
                edges: Vec::new(),
            },
        ];
        add("wire_vertex_offset", i);
    }

    // HaveTangentGaps with no samples: upstream evaluates once at 0 / 0
    let mut i = base("cone.smb", Some("cone.smb"), 0.5);
    i.pairs = pairs(4, 1);
    i.ops = vec![OpIn::HaveTangentGaps {
        n: 0,
        off: h(1, K_EDGEUSE, 0),
        other_off: h(1, K_EDGEUSE, 1),
    }];
    add("cone_have_tangent_gaps_no_samples", i);

    out
}

/// Bases of the `*_zero_distance` cases: their compared output changes at `dist = 0`.
///
/// A sweep of the 800 generated scripts at `dist = 0` (the `Getters` echo of the header
/// left out) changes 67 compared results, every one a script with a `FaceuseOffset` (the
/// offset surface at distance 0) or a `VertexOffset` (the radius-0 arc, status 1007);
/// `EdgeuseOffset` reads the distance on its rail path only, which defers. The built-in
/// `cone_vertex_offset` is the only built-in whose compared output changes. `gen0638`
/// offsets a B-spline face (`ellips_ts2.smb`); `gen0700` and `gen0012` offset planes.
const ZERO_DISTANCE_BASES: &[&str] = &[
    "cone_vertex_offset",
    "gen0700",
    "gen0107",
    "gen0012",
    "gen0638",
];

/// Bases of the `*_zero_approx` cases: their compared output changes at `approx = 0`.
///
/// The same sweep at `approx = 0` changes 2 compared results, both through
/// `VertexOffset`; the built-in `cone_trim_extended_edge` is the only built-in whose
/// compared output changes (`TrimExtendedEdge`'s `d3DTol`, ten times the tolerance).
const ZERO_APPROX_BASES: &[&str] = &["cone_trim_extended_edge", "gen0775", "gen0651"];

/// The run header at zero (maintainer review of #156, second round): the bases above with
/// `dist = 0` or `approx = 0`, named by exact case name (a generated base is taken from
/// the case list, so `MIN_GENERATED` keeps it present); a base that is missing is an error.
fn zero_header_cases(unit: &str, cases: &[Case]) -> Result<Vec<Case>, HarnessError> {
    let mut out = Vec::new();
    for (suffix, bases, zero_dist) in [
        ("zero_distance", ZERO_DISTANCE_BASES, true),
        ("zero_approx", ZERO_APPROX_BASES, false),
    ] {
        for base in bases {
            let Some(c) = cases.iter().find(|c| c.name == *base) else {
                return Err(harness(format!("zero-header base {base} is not a case")));
            };
            let mut i: Inputs = c.parse()?;
            if zero_dist {
                i.dist = 0.0;
            } else {
                i.approx = 0.0;
            }
            let inputs = serde_json::to_value(i).map_err(|e| harness(e.to_string()))?;
            out.push(Case {
                unit: unit.to_string(),
                name: format!("{base}_{suffix}"),
                source: CaseSource::Inline,
                inputs,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn import_input(name: &str) -> Inputs {
        Inputs {
            brep_a: file(name),
            brep_b: Some(file(name)),
            dist: 0.1,
            approx: 1e-5,
            analytics: true,
            local: Vec::new(),
            inset: false,
            extend: false,
            shell: Vec::new(),
            pairs: Vec::new(),
            ops: vec![OpIn::Getters],
        }
    }

    fn nurb_state(surface: &Surface) -> String {
        let base = match surface.kind() {
            SurfaceKind::BSpline(s) => Some(s),
            SurfaceKind::Cone(s) => Some(s.rev().base()),
            SurfaceKind::Sphere(s) => Some(s.base().base()),
            SurfaceKind::Torus(s) => Some(s.base().base()),
            SurfaceKind::SurfOfRevolution(s) => Some(s.base()),
            SurfaceKind::SurfOfExtrusion(s) => Some(s.base()),
            _ => None,
        };
        format!("{:?}", base.and_then(BSplineSurface::gw_nurb))
    }

    #[test]
    fn surface_records_restore_native_kinds_owners_tolerances_and_nurbs_in_both_arenas() {
        for (fixture, expected_type) in [
            (file("cone.smb"), CONE_TYPE),
            (file("Solids/cylinder.smb"), CONE_TYPE),
            (file("sphere.smb"), SPHERE_TYPE),
            (file("torus.smb"), SURF_OF_REVOLUTION_TYPE),
            (
                BrepIn::Cylinder {
                    radius: 2.0,
                    height: 3.0,
                },
                CYLINDER_TYPE,
            ),
            (
                BrepIn::Torus {
                    major: 3.0,
                    minor: 1.0,
                },
                TORUS_TYPE,
            ),
            (file("surf_extrude.smb"), SURF_OF_EXTRUSION_TYPE),
            (file("surf_rev.smb"), SURF_OF_REVOLUTION_TYPE),
        ] {
            let name = format!("{fixture:?}");
            let mut input = import_input("cone.smb");
            input.brep_a = fixture.clone();
            input.brep_b = Some(fixture);
            let out = export(&input).unwrap();
            let (a, b) = ffi::split_exports(&out).unwrap();
            let records = ffi::split_surface_records(&out).unwrap();
            for (block, records) in [a, b.unwrap()].iter().zip(&records) {
                let (mut brep, mut exact, types) = import_with_types(block).unwrap();
                assert!(types.contains(&Some(expected_type)), "{name}: {types:?}");
                let before: Vec<_> = brep
                    .faces()
                    .iter()
                    .map(|(_, face)| {
                        face.surface
                            .as_ref()
                            .map(|s| (s.owner(), s.owner_zone_tol_3d(), nurb_state(s)))
                    })
                    .collect();
                read_face_surfaces(&mut brep, &mut exact, &types, records).unwrap();
                assert!(exact.iter().all(|&value| value), "{name}: {exact:?}");
                for ((index, face), original) in brep.faces().iter().zip(before) {
                    let Some(surface) = face.surface.as_ref() else {
                        assert!(original.is_none());
                        continue;
                    };
                    let (owner, tolerance, nurb) = original.unwrap();
                    assert_eq!(surface.owner(), owner, "{name}: {index:?}");
                    assert_eq!(surface.owner_zone_tol_3d(), tolerance, "{name}: {index:?}");
                    assert_eq!(nurb_state(surface), nurb, "{name}: {index:?}");
                    if let Some(type_code) = types[index.index() as usize] {
                        assert_eq!(surface.type_code(), type_code, "{name}: {index:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn restored_surface_uses_its_own_face_tolerance_and_keeps_unowned_surfaces_unowned() {
        let out = export(&import_input("cone.smb")).unwrap();
        let (a, b) = ffi::split_exports(&out).unwrap();
        let records = ffi::split_surface_records(&out).unwrap();
        for (arena, (block, records)) in [a, b.unwrap()].iter().zip(&records).enumerate() {
            for unowned in [false, true] {
                let (mut brep, mut exact, types) = import_with_types(block).unwrap();
                let faces: Vec<_> = brep.faces().ids().collect();
                for &face_id in &faces {
                    let face = brep.face_mut(face_id).unwrap();
                    face.tolerance =
                        (arena + 1) as f64 * (f64::from(face_id.index()) + 1.0) * 0.003;
                    if unowned {
                        face.surface.as_mut().unwrap().set_owner(None);
                    }
                    brep.sync_face_surface_tolerance(face_id).unwrap();
                }
                read_face_surfaces(&mut brep, &mut exact, &types, records).unwrap();
                for face_id in faces {
                    let face = brep.face(face_id).unwrap();
                    let surface = face.surface.as_ref().unwrap();
                    assert_eq!(surface.owner().is_none(), unowned);
                    assert_eq!(
                        surface.face_zone_tol_3d().unwrap(),
                        (!unowned).then_some(face.tolerance),
                        "arena={arena}, face={face_id:?}",
                    );
                }
            }
        }
    }

    #[test]
    fn cache_context_distinguishes_equal_face_indices_in_two_arenas() {
        let surface = SrfIn::Bspline(BssIn {
            oob: false,
            form: 11,
            net: vec![
                vec![[0.0, 0.0, 0.0, NOW], [0.0, 1.0, 0.0, NOW]],
                vec![[1.0, 0.0, 0.0, NOW], [1.0, 1.0, 0.0, NOW]],
            ],
            p: 1,
            q: 1,
            u: vec![0.0, 0.0, 1.0, 1.0],
            v: vec![0.0, 0.0, 1.0, 1.0],
        });
        let fixture = BrepIn::Face {
            surface,
            dom: [0.0, 0.0, 1.0, 1.0],
        };
        let mut input = import_input("cone.smb");
        input.brep_a = fixture.clone();
        input.brep_b = Some(fixture);
        let mut prepared = prepare(&input).unwrap();
        let k = Kernel::default();
        for (arena, tolerance) in [(0, 0.003), (1, 0.007)] {
            let handle = hd(arena, K_FACE_SURFACE, 0);
            let brep = prepared.two.brep_m(handle).unwrap();
            let face = FaceId::from_index(0);
            brep.face_mut(face).unwrap().tolerance = tolerance;
            brep.sync_face_surface_tolerance(face).unwrap();
            // Exercise the native editing-mode cache branch, not guessed defaults.
            brep.set_editing_enabled(true);
            let source = ClassGeometry::surface(&prepared.two, handle).unwrap();
            let mut surface = source.clone();
            surface.set_owner(source.owner());
            surface.set_owner_zone_tol_3d(source.owner_zone_tol_3d());
            let cache = prepared
                .two
                .prepare_surface_solve_cache(&k, &surface, Some(handle))
                .unwrap();
            assert_eq!(cache.face().unwrap(), Some(hd(arena, K_FACE, 0)));
            assert_eq!(cache.surface().owner_zone_tol_3d(), Some(tolerance));
            assert_eq!(
                prepared
                    .two
                    .prepared_face_queries()
                    .unwrap()
                    .face_zone_tol_3d(hd(arena, K_FACE, 0))
                    .unwrap(),
                tolerance
            );
        }
    }

    #[test]
    fn incomplete_or_misaligned_surface_records_are_harness_errors_not_deferrals() {
        let out = export(&import_input("torus.smb")).unwrap();
        let (a, _) = ffi::split_exports(&out).unwrap();
        let [records, _] = ffi::split_surface_records(&out).unwrap();
        let index = records.iter().position(Option::is_some).unwrap();
        for mutation in 0..5 {
            let (mut brep, mut exact, mut types) = import_with_types(&a).unwrap();
            let mut records = records.clone();
            match mutation {
                0 => {
                    records.pop();
                }
                1 => {
                    records[index].as_mut().unwrap().pop();
                }
                2 => records[index].as_mut().unwrap().push(0),
                3 => records[index] = None,
                _ => types[index] = Some(SPHERE_TYPE),
            }
            assert!(read_face_surfaces(&mut brep, &mut exact, &types, &records).is_err());
        }
    }

    #[test]
    fn exact_revolution_import_keeps_the_native_self_intersection_hang_guard() {
        for name in ["vase_rev.smb", "toruswvy.smb"] {
            let mut input = import_input(name);
            input.brep_b = None;
            let prepared = prepare(&input).unwrap();
            let brep = prepared.two.breps[0].as_ref().unwrap();
            let faceuse = brep
                .faceuses()
                .iter()
                .find_map(|(id, faceuse)| {
                    let surface = brep.face(faceuse.face?).ok()?.surface.as_ref()?;
                    (surface.type_code() == SURF_OF_REVOLUTION_TYPE).then_some(id)
                })
                .unwrap();
            input.ops = vec![OpIn::FaceuseOffset {
                fu: h(0, K_FACEUSE, i64::from(faceuse.index())),
                skip: false,
            }];
            let rust = run_rust(&input).unwrap();
            assert_eq!(rust.deferred.unwrap().unit, HANG_26);
            let cpp = run_cpp(&input).unwrap();
            assert_eq!(cpp.values["state"], [0.0, 7.0, -6.0]);
        }
    }
}
