//! Runner for unit `topology.edge`. Ported from upstream `SmEdge.cpp`; checked against its
//! golden. Rust: `solidcore::topology::edge`.
//!
//! The case's B-rep is one of the reference shim's sources: read from an upstream `.smb`
//! test file (optionally rebuilding its UV trim curves), a wire edge from a B-spline curve
//! (`CreateWireEdgeFromCurve`), a line or ellipse/circle wire, a face from a B-spline
//! surface or a plane (`CreateFaceFromSurface`), or the wire network of `prog_test`
//! `my_test_topology`. [`own_brep`] builds it with the Rust kernel alone (the Rust `.smb`
//! reader, [`crate::own_builder`] and this runner's line and ellipse wires), renumbered
//! into the shim's traversal order, with the shim's `init_gaps` walk. Queries are
//! recorded as traversal indices, flags, statuses and values; edits (tolerances,
//! orientations, list changes, notifications, curve changes) by the mutable graph state
//! afterwards (the shim's `put_state`) and the curves they touch.
//!
//! Selectors are taken modulo the entity count (`-1` selecting null where allowed); the
//! sector of `AddPair` and the edge use of `ResetPrimary` are taken from the edge's own
//! list.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::{CurveKind, BSPLINE_CURVE_TYPE, LINE_TYPE};
use solidcore::curve::curve::Curve as SmCurve;
use solidcore::curve::line::Line;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::attribute::{Attribute, AttributeStore, Behavior, ObjectKey};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::object::{NotifyOperation, ObjectRef};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::topology::edge as ed;
use solidcore::topology::{
    Brep, EdgeId, Edgeuse, EdgeuseId, FaceId, FaceuseId, LoopId, LoopuseId, RegionId, ShellId,
    VertexId, VertexuseId,
};
use solidcore::{EntityRef, SolidError};

use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The unit runner.
pub struct Runner;

/// Largest edge sequence of `OrientSeq`.
const MAX_SEQ: usize = 16;

/// Largest derivative array of `Ode`.
const MAX_ODE: usize = 64;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}


/// The type codes `IsKindOf` is asked about (as the shim's `KIND_TYPES`).
const KIND_TYPES: [i64; 9] = [16016, 16005, 16001, 16010, 12060, 11001, 16013, 16021, 16999];

/// "Upstream would dereference null" in an output slot.
const NULL_DEREF: f64 = -2.0;

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
    /// An actual native ellipse/circle, with independent STEP and NURB intervals.
    Ellipse {
        circle: bool,
        center: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        angles: [f64; 2],
        radii: [f64; 2],
        inside_out: bool,
        natural: [f64; 2],
        fractions: [f64; 2],
        oob: bool,
        tolerance: f64,
        orientation: u8,
    },
}

// ---- encoding ---------------------------------------------------------------------------

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

/// Resolves a selector modulo `n` (`-1` and an empty kind select null).
fn sel(s: i64, n: usize) -> i64 {
    if s < 0 || n == 0 {
        -1
    } else {
        (s as u64 % n as u64) as i64
    }
}

// ---- import -----------------------------------------------------------------------------

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
            file: (*rng.choose(&FILES).unwrap_or(&"SmallBox.smp")).to_string(),
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
    /// `IsOnFaceEdgeForm(&dMaxGap3d)` (`SmEdge::AssertValid` test 13) after translating the
    /// edge curve by `d` (`SmCurve::Transform`; no move for zero).
    EdgeForm {
        e: i64,
        #[serde(default)]
        d: [f64; 3],
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
            edges: edges.into_iter().take(MAX_SEQ).map(es).collect(),
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
            n: n.clamp(28, MAX_ODE as u64),
        },
        OpIn::SetInterval { e, a, b } => OpIn::SetInterval { e: es(e), a, b },
        OpIn::EdgeForm { e, d } => OpIn::EdgeForm { e: es(e), d },
    })
}

// ---- C++ side ---------------------------------------------------------------------------

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
        Err(up_err) if solidcore::error::upstream_status(up_err).is_some() => Ok(Some(f64::from(solidcore::error::upstream_status(up_err).unwrap_or(-1)))),
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

/// The shim's operation-result curve block (`-1` for none).
fn put_curve(out: &mut Vec<f64>, c: Option<&SmCurve>) -> Result<(), HarnessError> {
    let Some(c) = c else {
        out.push(-1.0);
        return Ok(());
    };
    let (ty, base) = match c.kind() {
        CurveKind::BSpline(b) => (BSPLINE_CURVE_TYPE, b),
        CurveKind::Line(l) => (LINE_TYPE, l.base()),
        CurveKind::Ellipse(e) => (e.type_code(), e.base()),
        _ => return Err(harness("put_curve: unsupported curve kind")),
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
    if let CurveKind::Ellipse(e) = c.kind() {
        let pos = e.position();
        for p in [pos.origin(), pos.x_axis(), pos.y_axis()] {
            out.extend([p.x, p.y, p.z]);
        }
        let d = e.step_interval();
        out.extend([
            d.min(),
            d.max(),
            e.x_radius(),
            e.y_radius(),
            flag(e.inside_out()),
            flag(base.is_out_of_bounds_enabled()),
            base.knot_type() as u64 as f64,
            base.form() as u64 as f64,
        ]);
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
    let (mut b, c) = match own_brep(i)? {
        Ok(x) => x,
        Err(o) => return Ok(o),
    };
    let Some(op) = resolve(&i.op, c, &b) else {
        return Ok(Output::err("no_edge"));
    };
    match run_rust_op(&mut b, &op) {
        Ok(o) => Ok(o),
        Err(Fail::Solid(e)) => rust_err(&e),
        Err(Fail::Harness(h)) => Err(h),
    }
}

// ---- Rust-side B-rep ----------------------------------------------------------------------

/// The shim's build conventions: its `walk` follows the vertex use lists after the
/// loops' loop uses, its `init_gaps` walk calls `GetVertexEdgeGap` where a vertex use
/// has one, and owned geometry carries the identities this runner's import gave it (a
/// face `(SmFace_TYPE, 1 + index)`, an edge [`key_of`]), the B-rep's naming left
/// `Unknown`.
const CONVENTIONS: crate::own_builder::Conventions<'static> = crate::own_builder::Conventions {
    vertex_edge_gaps: true,
    vertex_lists_last: true,
    face_owner: &face_owner,
    edge_owner: &edge_owner,
    // the geometry is owned under this runner's own identities
    naming: solidcore::topology::NamingScheme::Unknown,
    ..crate::own_builder::Conventions::EDGEUSE_GRAPH
};

fn face_owner(f: FaceId) -> ObjectRef {
    ObjectRef {
        type_code: solidcore::topology::face::p2::TYPE,
        key: ObjectKey(1 + u64::from(f.index())),
    }
}

fn edge_owner(e: EdgeId) -> (ObjectRef, ObjectRef) {
    // the B-rep part only feeds the owner-change notification, which the copy drops
    (
        key_of(EntityRef::Edge(e)),
        ObjectRef {
            type_code: 16011,
            key: ObjectKey(1),
        },
    )
}

/// The Rust B-rep of the case and its entity counts, or the output of a failed build,
/// made by the Rust kernel alone as the shim's `build` makes it: an `.smb` file by the
/// Rust reader, the wire, face and network sources by [`crate::own_builder`], the
/// `SmLine` and ellipse wires by [`line_wire`] and [`ellipse_wire`]; then renumbered
/// into the shim's traversal order ([`CONVENTIONS`]). No C++ state reaches the Rust
/// side.
fn own_brep(i: &Inputs) -> Result<Result<(Brep, [usize; 10]), Output>, HarnessError> {
    use crate::own_builder::{build_as, counts, failed_output, finish, in_new_brep, read_as, Built};
    let mut k = Kernel::default();
    let (vo, ig) = (i.validate_only, i.init_gaps);
    let built = match &i.brep {
        BrepIn::File { rebuild, .. } => {
            read_as(&mut k, &path_of(&i.brep)?, *rebuild, vo, ig, &CONVENTIONS)?
        }
        BrepIn::Line { .. } | BrepIn::Ellipse { .. } => {
            let raw = in_new_brep(&mut k, |k, b| match &i.brep {
                BrepIn::Line {
                    p,
                    v,
                    a,
                    b: c,
                    scale,
                    inside_out,
                    f0,
                    f1,
                } => line_wire(k, b, (*p, *v), [*a, *c], *scale, *inside_out, [*f0, *f1]),
                _ => ellipse_wire(k, b, &i.brep),
            })?;
            finish(&mut k, raw, vo, ig, &CONVENTIONS)?
        }
        src => build_as(&mut k, src, vo, ig, &CONVENTIONS)?
            .ok_or_else(|| harness("own builder: unexpected file source"))?,
    };
    Ok(match built {
        Built::Brep(b) => {
            let c = counts(&b);
            Ok((*b, c))
        }
        Built::Failed(e) => Err(failed_output(&e)),
    })
}

/// Kind 4: `CreateWireEdgeFromCurve` on a real `SmLine(p, v, [a, b], scale, 3, ctx,
/// NULL, inside_out)` over the fractions `f` of its natural interval.
fn line_wire(
    k: &mut Kernel,
    b: &mut Brep,
    (p, v): ([f64; 3], [f64; 3]),
    [a, c]: [f64; 2],
    scale: f64,
    inside_out: bool,
    [f0, f1]: [f64; 2],
) -> Result<(), SolidError> {
    use solidcore::curve::line::BoundedLineOptions;
    let l = Line::bounded(
        Vec3::from_array(p),
        Vec3::from_array(v),
        &Extent1d::from_min_max(a, c),
        &BoundedLineOptions {
            scale,
            dim: 3,
            nurb: None,
            inside_out,
        },
    );
    let nat = l.natural_interval();
    solidcore::topology::brep::p5::create_wire_edge_from_curve(
        k,
        b,
        SmCurve::from(l),
        Extent1d::from_min_max(nat.evaluate(f0), nat.evaluate(f1)),
    )
    .map(|_| ())
}

/// Kind 5: a real `SmCircle` / `SmEllipse` over `angles`, its NURB reparameterised to
/// `natural` (`SM_ERR` without a NURB), out-of-bounds evaluation set, then
/// `CreateWireEdgeFromCurve` over the fractions of its natural interval and, on success,
/// the edge's `SetTolerance(tolerance, FALSE, FALSE)` and `SetOrientation`.
fn ellipse_wire(k: &mut Kernel, b: &mut Brep, src: &BrepIn) -> Result<(), SolidError> {
    use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
    let BrepIn::Ellipse {
        circle,
        center,
        x,
        y,
        angles: [a, c],
        radii: [rx, ry],
        inside_out,
        natural: [n0, n1],
        fractions: [f0, f1],
        oob,
        tolerance,
        orientation,
    } = src
    else {
        return Err(SolidError::invalid_input("topology_edge::ellipse_wire", "source"));
    };
    let (center, x, y) = (Vec3::from_array(*center), Vec3::from_array(*x), Vec3::from_array(*y));
    let domain = Extent1d::from_min_max(*a, *c);
    let opts = EllipseOptions {
        inside_out: *inside_out,
        ..EllipseOptions::default()
    };
    let mut curve = if *circle {
        Ellipse::new_circle(k, center, x, y, domain, *rx, &opts)?
    } else {
        Ellipse::new(k, center, x, y, domain, *rx, *ry, &opts)?
    };
    if curve.base().gw_nurb().is_none() {
        return Err(SolidError::sm_err("topology_edge::ellipse_wire"));
    }
    curve.edit_parameterization(k, &Extent1d::from_min_max(*n0, *n1), true, None)?;
    curve.set_out_of_bounds_enabled(*oob);
    let curve = SmCurve::from(curve);
    let natural = curve.natural_interval();
    let e = solidcore::topology::brep::p5::create_wire_edge_from_curve(
        k,
        b,
        curve,
        Extent1d::from_min_max(natural.evaluate(*f0), natural.evaluate(*f1)),
    )?;
    ed::set_tolerance(b, e, *tolerance, false, false)?;
    ed::set_orientation(b, e, orient_of_code(*orientation))
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
        OpIn::EndTangent { e, at_start } => match ed::end_tangent(&k, b, eid(*e), *at_start) {
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
                _ => ed::max_down_dim_gap_3d(&k, b, eid(*e), *want, t),
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
            let mut records = solidcore::topology::topology::NotifyRecords::new();
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
            let mut records = solidcore::topology::topology::NotifyRecords::new();
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
                Err(up_err) if solidcore::error::upstream_status(&up_err).is_some() => out.extend([f64::from(solidcore::error::upstream_status(&up_err).unwrap_or(-1)), -1.0, -1.0]),
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
            match ed::uv_trim_curve_of_surface(&k, b, eid(*e), opt_id(*f, FaceId::from_index)) {
                Ok(h) => out.extend([flag(h.is_some()), ix(h, EdgeuseId::index)]),
                Err(SolidError::InvalidInput { .. }) => out.push(NULL_DEREF),
                Err(err) => return Err(err.into()),
            }
        }
        OpIn::Seam { e, f } => {
            let r = match opt_id(*f, FaceId::from_index) {
                Some(f) => ed::is_seam_of_surface(&k, b, eid(*e), f),
                None => ed::is_seam(&k, b, eid(*e)),
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
        } => out.push(slot(ed::is_tangent_edge(&k, b, eid(*e), *tol, *coincidence), flag)?),
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
            match ode.evaluate(&k, *t, &mut dydx) {
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
        OpIn::EdgeForm { e, d } => {
            let e = eid(*e);
            let faces = ed::faces(b, e, None)?;
            // the shim's guard: the gap calculation dereferences the curve and surfaces
            let mut ok = faces.is_empty() || b.edge(e)?.curve.is_some();
            for fc in &faces {
                if b.face(*fc)?.surface.is_none() {
                    ok = false;
                }
            }
            if !ok {
                out.push(NULL_DEREF);
                return Ok(Output::new().values("state", out));
            }
            let mut moved = 0.0;
            if d.iter().any(|&x| x != 0.0) {
                if let Some(curve) = b.edge_mut(e)?.curve.as_mut() {
                    let mut place = Axis2Placement::default();
                    place.translate(Vec3::new(d[0], d[1], d[2]));
                    match curve.transform(&k, &place, None) {
                        Ok(()) => {}
                        Err(err) if err.is_unported() => return Err(err.into()),
                        Err(_) => moved = 1.0,
                    }
                }
            }
            out.push(moved);
            let (ok, gap) = ed::edge_form::is_on_face_edge_form(&k, b, e)?;
            out.extend([flag(ok), gap]);
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

