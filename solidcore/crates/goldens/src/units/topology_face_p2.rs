//! Runner for unit `topology.face_p2`. Ported from upstream `SmFace.cpp`; checked against
//! its golden. Rust: `solidcore::topology::face::p2`.
//!
//! **B-rep ops.** The case's B-rep is one of the `topology.vertexuse` runner's sources:
//! read from an upstream `.smb` test file (optionally rebuilding its UV trim curves), a
//! wire edge from a B-spline curve (`CreateWireEdgeFromCurve`), a face from a B-spline
//! surface or a plane (`CreateFaceFromSurface`), or the wire network of `prog_test`
//! `my_test_topology`. [`own_brep`] makes it with the Rust kernel (an `.smb` file read by
//! the Rust reader, any other kind built by [`crate::own_builder`], in the shim's call
//! order), renumbered into the shim's traversal order ([`CONVENTIONS`]); the runner runs
//! the ported functions on it and records traversal indices, flags, statuses, values and
//! curve blocks (the shim's `put_curve` layout). Selectors are taken modulo the entity
//! count (`-1` selecting null where allowed). The generator redraws the B-rep (a few
//! times) when it has none of the entity kind the op selects.
//!
//! **Input contract.** A case's encoding must keep the reference wrapper's contract
//! ([`keeps_contract`]); one outside it is a harness error.
//!
//! **`GetPointsInFace` with more than one point** draws its V dither from
//! `srand(time(NULL))` / `rand()` upstream and from `kernel.rng` in the port
//! (`plan/findings.md`); such cases record what does not depend on the draw: the
//! point count, the U values and, on a rectangular face, each V's grid slot
//! (`round(v / dVInc)`, the dither being under half a slot).
//!
//! **Geometry ops** (`sm_CheckSeamCrossing`, `SmPrecisePropertyLayout`,
//! `SmODEIsoCurveLengthIFEO::Evaluate`, `SmODEAreaIFEO::Evaluate`) run on B-spline
//! surfaces, planes and B-spline UV curves.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::CurveKind;
use solidcore::curve::curve::Curve as SmCurve;
use solidcore::geom::attribute::ObjectKey;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::object::ObjectRef;
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
use solidcore::surface::surface::Surface;
use solidcore::topology::face::p2 as fp2;
use solidcore::topology::{Brep, EdgeId, EdgeuseId, Face, FaceId, FaceuseId, LoopId, VertexId};
use solidcore::SolidError;

use crate::cases::contract;
use crate::cases::topology_entities::Reader;
use crate::{Case, CaseContext, HarnessError, Output, Rng, UnitDiff};

/// The unit runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}


/// The type codes `IsKindOf` is asked about (as the shim's `KIND_TYPES`).
const KIND_TYPES: [i64; 9] = [16002, 16005, 16001, 16010, 12060, 11001, 16013, 16022, 16999];

/// "Upstream would dereference null" in an output slot.
const NULL_DEREF: f64 = -2.0;

/// Generated cases beyond the default `--seeds` (the B-rep ops are cheap).
const EXTRA_GENERATED: usize = 1500;

/// Times a generated case's B-rep is drawn again, at most, when it lacks the entity kind its
/// op selects (a wire or an edge network for a face op).
const BREP_DRAWS: usize = 8;

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
                b.u.len().checked_sub(1).ok_or_else(|| harness("empty u knots"))? as f64,
                b.v.len().checked_sub(1).ok_or_else(|| harness("empty v knots"))? as f64,
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


// ---- import -----------------------------------------------------------------------------

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
    match rng.below(10) {
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

// ---- the Rust B-rep ---------------------------------------------------------------------

/// The shim's build conventions (its own `build` and `walk`): the vertex use lists
/// followed after the loops' loop uses, `GetVertexEdgeGap` in the `init_gaps` walk; a
/// face's surface owned under the identity [`import`] gives it (`(SmFace_TYPE, 1 +
/// index)`), an edge's curve under the `sc_topology_fixture.h` identity. Shared with the
/// runners whose shims build and walk B-reps the same way (`query.face_p3`,
/// `query.face_p4`, `surface.trim_srf_cache`).
pub(crate) const CONVENTIONS: crate::own_builder::Conventions<'static> =
    crate::own_builder::Conventions {
        prefix_lists: false,
        vertex_edge_gaps: true,
        vertex_lists_last: true,
        skip_edge_list: false,
        face_owner: &face_owner,
        edge_owner: &edge_owner,
        edgeuse_owner: &crate::own_builder::notify_edgeuse_owner,
        // the geometry is owned under this runner's own identities
        naming: solidcore::topology::NamingScheme::Unknown,
    };

fn face_owner(f: FaceId) -> ObjectRef {
    ObjectRef {
        type_code: fp2::TYPE,
        key: ObjectKey(1 + u64::from(f.index())),
    }
}

fn edge_owner(e: EdgeId) -> (ObjectRef, ObjectRef) {
    use crate::own_builder::fixture_key;
    (
        fixture_key(solidcore::topology::edge::TYPE, e.index()),
        fixture_key(16999, 0),
    )
}

/// A Rust B-rep with its entity counts, or the output of its failed build.
pub(crate) type OwnBrep = crate::own_builder::RustBrep<(Brep, [usize; 10])>;

/// A built (or failed) source as the runners consume it.
#[allow(clippy::result_large_err)] // the failed build's output is the case's result
pub(crate) fn own_output(built: crate::own_builder::Built) -> OwnBrep {
    use crate::own_builder::{counts, failed_output, Built};
    match built {
        Built::Brep(b) => {
            let c = counts(&b);
            Ok((*b, c))
        }
        Built::Failed(e) => Err(failed_output(&e)),
    }
}

/// The Rust B-rep of a shim B-rep source, made by the Rust kernel as the shim's `build`
/// makes it, in its traversal order ([`CONVENTIONS`]): an `.smb` file (`path`) read by
/// the Rust reader, any other kind built by [`crate::own_builder`]. `src` is any
/// `kind`-tagged source whose JSON has a [`BrepIn`]'s shape.
///
/// # Errors
///
/// A harness error for a source the builders do not know or a file that cannot be
/// opened.
pub(crate) fn own_brep<T: Serialize>(
    k: &mut Kernel,
    src: &T,
    path: &str,
    validate_only: bool,
    init_gaps: bool,
) -> Result<OwnBrep, HarnessError> {
    let v = serde_json::to_value(src).map_err(|e| harness(format!("B-rep source: {e}")))?;
    if v.get("kind").and_then(Value::as_str) == Some("file") {
        let rebuild = v.get("rebuild").and_then(Value::as_bool).unwrap_or(false);
        let built = crate::own_builder::read_as(
            k,
            path,
            rebuild,
            validate_only,
            init_gaps,
            &CONVENTIONS,
        )?;
        return Ok(own_output(built));
    }
    let built = crate::own_builder::build_as(k, &v, validate_only, init_gaps, &CONVENTIONS)?
        .ok_or_else(|| harness("own builder: no file path"))?;
    Ok(own_output(built))
}

// ---- input contract -----------------------------------------------------------------------

/// The reference shim's operations (the code an op is encoded under).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Export = 0,
    FaceQuery = 1,
    Set = 2,
    Points = 3,
    Internal = 4,
    FromFace = 5,
    Trim = 6,
    FixUv = 7,
    BestUv = 8,
    Connected = 9,
    Rescue = 10,
    RemoveSuspect = 11,
    SeamCross = 20,
    Layout = 21,
    OdeIso = 22,
    OdeArea = 23,
}

/// Largest control point index.
const MAX_INDEX: usize = 32;
/// Largest degree.
const MAX_DEGREE: usize = 5;
/// Largest magnitude of any value (coordinates included). HANG-29: with a B-rep or
/// curve coordinate whose square overflows (from about `1.34e154`), the face build's
/// `SmCurve::GlobalCoincidenceChecker` (through `DropPoint`) splits forever; the bound
/// keeps Euclidean points (weights `1e-3..=1e3`) under that regime.
const MAX_COORD: f64 = 1.0e150;
/// Largest point count of `Points`.
const MAX_POINTS: usize = 4096;
/// Largest number of curves in a list argument.
const MAX_CURVES: usize = 8;
/// Largest derivative array of the ODE ops.
const MAX_DYDX: usize = 64;
/// Largest number of classification intervals of `RemoveSuspect`.
const MAX_INTERVALS: usize = 16;

/// `SmPrecisePropertyLayout(flags).m_lNumComponents`.
fn layout_components(flags: u64) -> usize {
    let mut n = 1;
    for (bit, count) in [(0x01, 1), (0x08, 1), (0x02, 3), (0x04, 9), (0x10, 3), (0x20, 9)] {
        if flags & bit != 0 {
            n += count;
        }
    }
    n
}

/// `k` control points sharing their rational marker (`z = NOZ` only for planar curves),
/// weights in `[1e-3, 1e3]` and finite coordinates within [`MAX_COORD`].
fn points_ok(rd: &mut Reader<'_>, k: usize, planar: bool) -> Option<()> {
    let mut w0 = None;
    for _ in 0..k {
        let [x, y, z, w] = [rd.num()?, rd.num()?, rd.num()?, rd.num()?];
        if z == NOZ && !planar {
            return None;
        }
        let rational = w != NOW;
        if *w0.get_or_insert(rational) != rational || (rational && !(1e-3..=1e3).contains(&w)) {
            return None;
        }
        let z = if z == NOZ { 0.0 } else { z };
        if [x, y, z].iter().any(|c| !c.is_finite() || c.abs() > MAX_COORD) {
            return None;
        }
    }
    Some(())
}

/// The parameter domain of a surface block that keeps the contract.
enum Domain {
    /// A plane: unbounded.
    Plane,
    /// A B-spline surface: `[u0, v0, u1, v1]`.
    Bounded([f64; 4]),
}

/// A surface block: a plane with a non-zero normal, or a B-spline surface within the size
/// bounds with clamped knots.
fn surface_ok(rd: &mut Reader<'_>) -> Option<Domain> {
    if rd.index(1)? == 1 {
        rd.vec3()?;
        rd.nonzero3()?;
        return Some(Domain::Plane);
    }
    rd.flag()?;
    rd.index(11)?;
    let n = rd.index(MAX_INDEX)?;
    let m = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let q = rd.index(MAX_DEGREE)?;
    let r = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    let s = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || q == 0 || q > m || r != n + p + 1 || s != m + q + 1 {
        return None;
    }
    points_ok(rd, (n + 1) * (m + 1), false)?;
    let u = rd.finites(r + 1)?;
    let v = rd.finites(s + 1)?;
    let (u0, u1) = contract::clamped(&u, p)?;
    let (v0, v1) = contract::clamped(&v, q)?;
    Some(Domain::Bounded([u0, v0, u1, v1]))
}

/// A B-spline curve block of dimension 2 or 3 within the size bounds with clamped knots;
/// its parameter range.
fn curve_ok(rd: &mut Reader<'_>) -> Option<(f64, f64)> {
    let dim = rd.index(3)?;
    if dim < 2 {
        return None;
    }
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return None;
    }
    points_ok(rd, n + 1, dim == 2)?;
    contract::clamped(&rd.finites(m + 1)?, p)
}

/// A B-rep block: an existing `.smb` file, a wire edge inside its curve's interval, a face
/// with an ordered domain inside its surface's (a plane's within `1e4`), or the network.
fn brep_block_ok(rd: &mut Reader<'_>, path: &str) -> Option<()> {
    match rd.index(3)? {
        0 => {
            let p = std::path::Path::new(path);
            if p.extension().and_then(|e| e.to_str()) != Some("smb") || !p.is_file() {
                return None;
            }
            rd.flag()?;
        }
        1 => {
            let (a, b) = curve_ok(rd)?;
            let t0 = rd.finite()?;
            let t1 = rd.finite()?;
            if !(a <= t0 && t0 < t1 && t1 <= b) {
                return None;
            }
        }
        2 => {
            let dom = surface_ok(rd)?;
            let d = rd.finites(4)?;
            let [a, b, c, e] = <[f64; 4]>::try_from(d.as_slice()).ok()?;
            if a >= c || b >= e {
                return None;
            }
            match dom {
                Domain::Bounded([u0, v0, u1, v1]) => {
                    if a < u0 || c > u1 || b < v0 || e > v1 {
                        return None;
                    }
                }
                Domain::Plane => {
                    if d.iter().any(|x| x.abs() > 1e4) {
                        return None;
                    }
                }
            }
        }
        _ => {}
    }
    rd.flag()?;
    rd.flag()?;
    Some(())
}

/// An index below `count`, or `-1`.
fn opt_below_ok(rd: &mut Reader<'_>, count: usize) -> Option<()> {
    let v = rd.num()?;
    (v == -1.0 || (v.fract() == 0.0 && v >= 0.0 && v < count as f64)).then_some(())
}

/// An optional curve (`0`, or `1` and a curve block of dimension `dim`); whether present.
fn opt_curve_ok(rd: &mut Reader<'_>, dim: f64) -> Option<bool> {
    if !rd.flag()? {
        return Some(false);
    }
    if rd.peek(0).unwrap_or(0.0) != dim {
        return None;
    }
    curve_ok(rd)?;
    Some(true)
}

/// The arguments after the B-rep block, against the B-rep's counts `c`.
fn brep_args_ok(op: Op, rd: &mut Reader<'_>, c: [usize; 10]) -> Option<()> {
    let [_, _, n_fu, n_f, _, n_l, n_eu, _, _, n_v] = c;
    match op {
        Op::Export => {}
        Op::FaceQuery => {
            rd.below(n_f)?;
            rd.flag()?;
            rd.flag()?;
            for _ in 0..rd.index(MAX_CURVES)? {
                rd.below(n_l)?;
            }
        }
        Op::Set => {
            rd.below(n_f)?;
            rd.finites(4)?;
            opt_below_ok(rd, n_fu)?;
        }
        Op::Points => {
            rd.below(n_f)?;
            rd.index(MAX_POINTS)?;
            if rd.flag()? {
                rd.finites(4)?;
            }
        }
        Op::Internal => {
            rd.below(n_f)?;
        }
        Op::FromFace => {
            rd.below(n_f)?;
            rd.index(2)?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::Trim => {
            rd.below(n_f)?;
            rd.flag()?;
            if !(0.0..=1.0e3).contains(&rd.finite()?) {
                return None;
            }
            let has_3d = rd.flag()?;
            let n3 = rd.index(MAX_CURVES)?;
            let mut all_3d = true;
            for _ in 0..n3 {
                all_3d &= opt_curve_ok(rd, 3.0)?;
            }
            let has_uv = rd.flag()?;
            let nuv = rd.index(MAX_CURVES)?;
            for _ in 0..nuv {
                opt_curve_ok(rd, 2.0)?;
            }
            let want_3d = rd.flag()?;
            rd.flag()?;
            // UV trimming indexes the 3D list in step with the UV list
            if has_uv && has_3d && want_3d && (n3 < nuv || !all_3d) {
                return None;
            }
        }
        Op::FixUv => {
            rd.below(n_eu)?;
            opt_below_ok(rd, n_eu)?;
            opt_below_ok(rd, n_v)?;
            rd.finite()?;
            rd.finite()?;
            rd.flag()?;
            rd.finites(4)?;
        }
        Op::BestUv => {
            rd.below(n_eu)?;
            for _ in 0..rd.index(MAX_CURVES)? {
                // 0 null, 1 a 2D curve block, 2 the edge use's own UV trim curve
                if rd.index(2)? == 1 {
                    if rd.peek(0).unwrap_or(0.0) != 2.0 {
                        return None;
                    }
                    curve_ok(rd)?;
                }
                rd.index(2)?;
            }
        }
        Op::Connected => {
            rd.below(n_eu)?;
            rd.below(n_eu)?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::Rescue => {
            rd.below(n_f)?;
            rd.flag()?;
        }
        Op::RemoveSuspect => {
            for _ in 0..2 * rd.index(MAX_INTERVALS)? {
                opt_below_ok(rd, n_v)?;
            }
        }
        Op::SeamCross | Op::Layout | Op::OdeIso | Op::OdeArea => return None,
    }
    Some(())
}

/// A geometry op's arguments.
fn geom_args_ok(op: Op, rd: &mut Reader<'_>) -> Option<()> {
    match op {
        Op::SeamCross => {
            rd.finites(8)?;
            rd.flag()?;
            rd.flag()?;
            rd.finites(2)?;
        }
        Op::Layout => {
            rd.index(255)?;
        }
        Op::OdeIso => {
            surface_ok(rd)?;
            rd.finites(5)?;
            if rd.finite()?.abs() > 1.0e3 {
                return None;
            }
            let flags = rd.index(63)?;
            let n = rd.index(MAX_DYDX)?;
            if n < 2 || n < layout_components(flags as u64) + 1 {
                return None;
            }
        }
        Op::OdeArea => {
            surface_ok(rd)?;
            let (a, b) = curve_ok(rd)?;
            let t = rd.finite()?;
            if !(a <= t && t <= b) {
                return None;
            }
            rd.finite()?;
            if !(1.0e-12..=1.0).contains(&rd.finite()?) {
                return None;
            }
            let ny = rd.index(MAX_DYDX)?;
            for _ in 0..ny {
                if rd.finite()? <= 0.0 {
                    return None;
                }
            }
            rd.finites(3)?;
            if rd.finite()?.abs() > 1.0e3 {
                return None;
            }
            let flags = rd.index(63)?;
            if ny < layout_components(flags as u64) {
                return None;
            }
            let n = rd.index(MAX_DYDX)?;
            rd.finites(n)?;
        }
        _ => return None,
    }
    Some(())
}

/// Whether an encoded op keeps the reference wrapper's input contract: a geometry op's
/// whole arguments, else the B-rep block (and, given the B-rep's counts `c`, the whole
/// arguments after it). A case outside it is a harness error: well-formed blocks of
/// finite values within [`MAX_COORD`] (one rationality per control net, HANG-29) are what
/// the builders and ops run on.
fn keeps_contract(op: Op, path: &str, input: &[f64], c: Option<[usize; 10]>) -> bool {
    let mut rd = Reader::new(input, MAX_COORD);
    if !op.is_brep() {
        return geom_args_ok(op, &mut rd).is_some() && rd.done();
    }
    if brep_block_ok(&mut rd, path).is_none() {
        return false;
    }
    match (op, c) {
        (Op::Export, _) => rd.done(),
        (_, Some(c)) => brep_args_ok(op, &mut rd, c).is_some() && rd.done(),
        (_, None) => true,
    }
}

impl Op {
    /// Whether the op starts with a B-rep block.
    fn is_brep(self) -> bool {
        (self as i32) < Op::SeamCross as i32
    }
}

/// The harness error of an input outside the reference wrapper's contract.
const CONTRACT: &str = "input outside the reference wrapper's contract";

fn counts_of(c: [usize; 10]) -> Counts {
    let [_, _, fu, f, _, l, eu, _, _, v] = c;
    Counts { fu, f, l, eu, v }
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
        CurveKind::Ellipse(e) => (std::borrow::Cow::Borrowed(e.base()), None),
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

fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    // an input outside the reference wrapper's contract is a harness error, so the
    // builders and ops run on well-formed blocks only. Nightly fuzz case fuzz00020 (run
    // 41040730716) made one control point of a non-rational face surface rational
    // (weight 1, the others the non-rational marker -f64::MAX), and the face built on
    // those weights ran `best_uv` past the fuzz time limit. The check sees a geometry
    // op's arguments; a B-rep op's B-rep block before the build, then its arguments
    // against the B-rep's counts.
    if !i.op.is_brep() {
        let (code, args) = op_args(&i.op)?;
        if !keeps_contract(code, "", &args, None) {
            return Err(harness(CONTRACT));
        }
        return match run_rust_geom(&i.op) {
            Ok(o) => Ok(geom_output(&i.op, o)),
            Err(Fail::Solid(e)) => rust_err(&e),
            Err(Fail::Harness(h)) => Err(h),
        };
    }
    let brep = i.brep.as_ref().ok_or_else(|| harness("B-rep op without B-rep"))?;
    let path = path_of(i)?;
    let block = brep_block(i)?;
    if !keeps_contract(Op::Export, &path, &block, None) {
        return Err(harness(format!("export: {CONTRACT}")));
    }
    let mut k = Kernel::default();
    let (mut b, c) = match own_brep(&mut k, brep, &path, i.validate_only, i.init_gaps)? {
        Ok(x) => x,
        Err(o) => return Ok(o),
    };
    let op = match resolve(&i.op, &counts_of(c)) {
        Ok(op) => op,
        Err(name) => return Ok(Output::err(name)),
    };
    let (code, args) = op_args(&op)?;
    let mut input = block;
    input.extend(args);
    if !keeps_contract(code, &path, &input, Some(c)) {
        return Err(harness(CONTRACT));
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
            Err(SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }) => o.push(NULL_DEREF),
            Err(e) => o.extend([status_of(e)?, 0.0]),
            }
        }
        OpIn::Internal { f } => {
            // the reference shim's `DeleteObjectsCache(SM_OC_TRIMSRF, srf)` before
            // `CalculateAnInternalPoint` (its OP_INTERNAL)
            super::query_face_p1::drop_trim_cache(b, fid(*f))?;
            match fp2::calculate_an_internal_point(&k, b, fid(*f)) {
            Ok(p) => o.extend([0.0, p.x, p.y, p.z]),
                Err(e) => o.extend([status_of(e)?, 0.0, 0.0, 0.0]),
            }
        }
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
            Err(SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }) => o.push(NULL_DEREF),
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
                Err(SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }) => o.push(NULL_DEREF),
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
                Err(SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }) => return Ok(vec![NULL_DEREF]),
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
                Err(SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }) => return Ok(vec![NULL_DEREF]),
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
            Err(SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }) => o.push(NULL_DEREF),
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

fn run_rust_geom(op: &OpIn) -> Result<Vec<f64>, Fail> {
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
            let k = Kernel::default();
            let r = fp2::check_seam_crossing(
                &k,
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
            // The derivative array is bounded (`MAX_DYDX`) so a fuzzed count cannot
            // make the runner allocate without bound.
            if *n > MAX_DYDX {
                return Err(harness(format!("bad index/count {n} (max {MAX_DYDX})")).into());
            }
            let s = rust_surface(surface)?;
            let l = fp2::PrecisePropertyLayout::new(*flags);
            let k = Kernel::default();
            let e = fp2::OdeIsoCurveLength::new(&k, &s, *u, Vec3::new(origin[0], origin[1], origin[2]), *thickness, &l);
            let mut d = vec![7.0; *n];
            let r = e.evaluate(*t, &mut d);
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
            match e.evaluate(&k, *t, &mut d) {
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
            let need = layout_components(flags) + 1;
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
            let need = layout_components(flags);
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
    let mut i = Inputs {
        brep,
        validate_only: rng.chance(0.1),
        init_gaps: rng.chance(0.7),
        op,
    };
    // A B-rep without the entity kind the op selects records only a refusal: draw
    // another. One that doesn't build, or no draw with the kind, keeps
    // the case as it is, so its error stays visible.
    for _ in 0..BREP_DRAWS {
        if !lacks_kind(&i) {
            break;
        }
        i.brep = Some(gen_brep(rng));
    }
    i
}

/// Whether `i`'s B-rep builds but has none of the entity kind its op selects.
fn lacks_kind(i: &Inputs) -> bool {
    let (Some(brep), Ok(path)) = (&i.brep, path_of(i)) else {
        return false;
    };
    let mut k = Kernel::default();
    own_brep(&mut k, brep, &path, i.validate_only, i.init_gaps)
        .is_ok_and(|b| b.is_ok_and(|(_, c)| resolve(&i.op, &counts_of(c)).is_err()))
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.face_p2"
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        run_rust(&case.parse::<Inputs>()?)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Nightly fuzz case `topology.face_p2/fuzz00020` (run 41040730716): a non-rational face
    /// surface (weights the marker `-f64::MAX`) with one weight set to 1, under a `best_uv`
    /// op, is refused instead of building the face on those weights.
    #[test]
    fn mixed_rationality_is_refused() {
        let i: Inputs = serde_json::from_str(FUZZ00020).unwrap();
        let rust = run_rust(&i).unwrap_err().to_string();
        assert_eq!(rust, format!("export: {CONTRACT}"));
    }

    /// The inputs of nightly fuzz case `topology.face_p2/fuzz00020`.
    const FUZZ00020: &str = r#"{"brep":{"dom":[0.0,0.0,1.0,1.0],"kind":"face","surface":{"bspline":{"form":0,"net":[[[0.0,0.0,0.48290885450886867,1.0],[0.0,1.0,0.026558355163892045,-1.7976931348623157e+308],[0.0,2.0,0.3236024010000994,-1.7976931348623157e+308]],[[1.0,0.0,0.16008492196929902,-1.7976931348623157e+308],[1.0,1.0,-0.3818595663202652,-1.7976931348623157e+308],[1.0,2.0,0.42438880397299017,-1.7976931348623157e+308]],[[2.0,0.0,-0.39686080956013436,-1.7976931348623157e+308],[2.0,1.0,0.05611566788706901,-1.7976931348623157e+308],[2.0,2.0,0.3283143419234992,-1.7976931348623157e+308]],[[3.0,0.0,0.363745333750945,-1.7976931348623157e+308],[3.0,1.0,0.36228231985820736,-1.7976931348623157e+308],[3.0,2.0,0.027268171389889018,-1.7976931348623157e+308]],[[4.0,0.0,-0.345322603754885,-1.7976931348623157e+308],[4.0,1.0,-0.32212985152181606,-1.7976931348623157e+308],[4.0,2.0,-0.4060463095725655,-1.7976931348623157e+308]]],"oob":false,"p":3,"q":1,"u":[0.0,0.0,0.0,0.0,0.5811054631449626,1.0,1.0,1.0,1.0],"v":[0.0,0.0,0.8672390488922626,1.0,1.0]}}},"cands":[{"curve":{"cpts":[[0.7895661339805444,-0.17171284016679556,1.7976931348623157e+308,-1.7976931348623157e+308],[0.8055199936793002,0.08681972422832687,1.7976931348623157e+308,-1.7976931348623157e+308],[0.7898024725911749,0.32205588150363834,1.7976931348623157e+308,-1.7976931348623157e+308]],"dim":2,"p":1,"u":[-1.5099557060603712,-1.5099557060603712,3.4113170103733608,5.537417838142818,5.537417838142818]},"kind":1,"orient":1},{"curve":{"cpts":[[0.41791217040037293,0.6071607649021995,-1e+300,1.846060688685256],[0.44522646681436484,0.38625390666912246,1.7976931348623157e+308,1.2398708818883835],[0.7494185757767893,0.6328648305934458,1.7976931348623157e+308,1.8680767293062457],[0.7900457641165108,0.4428112016048782,1.7976931348623157e+308,1.7317769440249347],[0.8780064788048215,0.3577346108988606,1.7976931348623157e+308,1.8293958767881109]],"dim":2,"p":3,"u":[0.0,0.0,0.0,0.0,0.6994601289283375,1.0,1.0,1.0,1.0]},"kind":1,"orient":1},{"curve":null,"kind":2,"orient":1}],"eu":870782,"init_gaps":true,"op":"best_uv","validate_only":false}"#;
}
