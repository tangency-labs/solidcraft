//! Runner for unit `topology.vertexuse`. Ported from upstream `SmVertexuse.cpp`; checked
//! against its golden. Rust: `solidcore::topology::vertexuse`.
//!
//! The case's B-rep is one of the reference shim's sources: read from an upstream `.smb`
//! test file (optionally rebuilding its UV trim curves), a wire edge from a B-spline curve
//! (`CreateWireEdgeFromCurve`), a face from a B-spline surface or a plane
//! (`CreateFaceFromSurface`), or the wire network of `prog_test` `my_test_topology`. The
//! runner builds a wire edge, face or network source itself ([`crate::own_builder`]) and
//! reads an `.smb` file with the Rust reader ([`crate::own_reader::file_in_order`]), both
//! renumbered into the shim's traversal order. It runs the ported functions on that
//! B-rep; their results are recorded as traversal indices, flags, statuses and values.
//! `init_gaps` computes the cached gaps before the op, so that resets are observable.
//!
//! The vertex use selector is taken modulo the vertex use count (so generated cases need
//! no counts); target selectors of `IsConnectedTo` likewise, `-1` selecting null.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::{NotifyOperation, ObjectRef};
use solidcore::kernel::Kernel;
use solidcore::nurbs::math::{NOW, NOZ};
use solidcore::topology::topology::Topology;
use solidcore::topology::vertexuse as vu1;
use solidcore::topology::{
    Brep, EdgeId, EdgeuseId, FaceId, FaceuseId,
    LoopId, LoopuseId, RegionId, ShellId, VertexId, VertexuseId,
};
use solidcore::{EntityRef, SolidError};

use crate::cases::topology_entities::source_magnitudes_ok;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The unit runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

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

/// Resolves a selector modulo `n` (`-1` and an empty kind select null).
pub(crate) fn sel(s: i64, n: usize) -> i64 {
    if s < 0 || n == 0 {
        -1
    } else {
        (s as u64 % n as u64) as i64
    }
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
        Err(SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }) => Ok(NULL_DEREF),
        Err(e) => Err(e),
    }
}

/// An upstream status: `0` for success, the `SmStatus` of an upstream error.
fn status_of(e: &SolidError) -> Result<f64, SolidError> {
    match e {
        up_err if solidcore::error::upstream_status(up_err).is_some() => Ok(f64::from(solidcore::error::upstream_status(up_err).unwrap_or(-1))),
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

/// A Rust B-rep and its entity counts.
type OwnBrep = (Brep, [usize; 10]);

/// The Rust B-rep of a case and its entity counts, or the output of a failed build or
/// read, under `sc_topology_fixture.h`'s conventions: shim-built sources built by the
/// Rust kernel ([`crate::own_builder`]), `.smb` file sources read by the Rust reader
/// ([`crate::own_reader::file_in_order`]), both renumbered into the shim's traversal
/// order. Every face surface is the Rust B-rep's own, exact, so no case is withheld as
/// an inexact import (the export's analytic surfaces no longer reach the Rust side).
fn rust_brep(
    i: &Inputs,
) -> Result<crate::own_builder::RustBrep<OwnBrep>, HarnessError> {
    use crate::own_builder::{build_as, counts, failed_output, Built, Conventions};
    let mut k = Kernel::default();
    let conv = &Conventions::TOPOLOGY_FIXTURE;
    let built = if let Some(built) = build_as(&mut k, &i.brep, i.validate_only, i.init_gaps, conv)?
    {
        built
    } else {
        let BrepIn::File { rebuild, .. } = &i.brep else {
            return Err(harness("own builder: a non-file source was not built"));
        };
        let path = path_of(&i.brep)?;
        crate::own_reader::file_in_order(
            &mut k,
            &path,
            *rebuild,
            i.validate_only,
            i.init_gaps,
            conv,
        )?
    };
    match built {
        Built::Brep(b) => {
            let c = counts(&b);
            Ok(Ok((*b, c)))
        }
        Built::Failed(e) => Ok(Err(failed_output(&e))),
    }
}

fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    // a B-rep value above 1e150 is refused (HANG-29, fuzz case fuzz00086): building a face on such a
    // surface does not return upstream and spins in the port for tens of seconds
    let source = serde_json::to_value(&i.brep).map_err(|e| harness(e.to_string()))?;
    if !source_magnitudes_ok(&source) {
        return Err(harness("B-rep value above 1e150 (HANG-29)"));
    }
    let (mut b, c) = match rust_brep(i)? {
        Ok(x) => x,
        Err(o) => return Ok(o),
    };
    let Some(op) = resolve(&i.op, c) else {
        return Ok(Output::err("no_vertexuse"));
    };
    // The ops read the cached gaps (`IsOnPole`: the vertex/face gap's face UV), so a
    // gap that cannot be refilled is reported, not left at its placeholder.
    if let Err(e) = recalc_gaps(&mut b) {
        return rust_err(&e);
    }
    match run_rust_op(&mut b, &op) {
        Ok(o) => Ok(o),
        Err(Fail::Solid(e)) => rust_err(&e),
        Err(Fail::Harness(h)) => Err(h),
    }
}

/// Recomputes every cached gap the B-rep marks as set (see [`fill_gaps`]); a gap left at
/// its `SM_UNDEF_DOUBLE` placeholder would be seen by an op reading a cache (`IsOnPole`:
/// the vertex/face gap's face UV). Stops at the first gap it cannot recompute (whose
/// caches stay partly refilled).
///
/// # Errors
///
/// That of the first failed recomputation.
pub(crate) fn recalc_gaps(b: &mut Brep) -> Result<(), SolidError> {
    fill_gaps(b)
}

/// The cached-gap recomputation of [`recalc_gaps`], as the fixture's `init_gaps` walk
/// fills them (vertex uses in arena order, which is the traversal order:
/// `GetVertexFaceGap`, then `GetVertexEdgeGap`; then edge uses: `GetMaxEdgeFaceGap`,
/// then `GetEdgeCCWEdgeGap`), through the upstream setters with each record stored where
/// they store it, but only into the caches the B-rep marks as set:
/// - `CalcVertexFaceGap`: the vertex use's own record.
/// - `CalcVertexEdgeGap`: one record, naming the vertex use that computed it, stored in
///   every use of the vertex on the same edge (the first set one in arena order
///   computes it; the walk's `GetVertexEdgeGap` of the others finds it cached). When
///   upstream computes it in another order (another walk, a healer), the record names
///   another use of that group and, at a closed edge, the other end parameter; its gap
///   length is the same.
/// - `CalcMaxEdgeFaceGap`: one record stored in the edge use and its mate (renamed to
///   the mate); the first set one in arena order computes it.
/// - `CalcEdgeCCWEdgeGap`: the edge use's own record.
///
/// Each record is stored only after it is fully computed; the first failure stops it.
fn fill_gaps(b: &mut Brep) -> Result<(), SolidError> {
    // The refill must not leave cache history the fixture's walk does not: its
    // smsurf_GetSurfaceCache calls build no tree (as during a Boolean operation: no UV
    // trim curves are made), and the trimmed caches they keep are dropped again, leaving
    // each surface's cache slot as a face-unaware lookup outside a Boolean operation
    // leaves it (a built tree).
    use solidcore::surface::surface::CacheTreeState;
    let before: Vec<Option<(bool, CacheTreeState)>> = b
        .faces()
        .iter()
        .map(|(_, f)| {
            f.surface
                .as_ref()
                .map(|s| (s.has_trim_cache(), s.cache_tree_state()))
        })
        .collect();
    let result = fill_gaps_body(b);
    for ((_, f), before) in b.faces().iter().zip(before) {
        if let (Some(s), Some((had, state))) = (f.surface.as_ref(), before) {
            if !had && s.has_trim_cache() && !s.trim_cache_in_use() {
                s.delete_trim_cache();
            }
            if state == CacheTreeState::NoCache && s.cache_tree_state() == CacheTreeState::Unbuilt
            {
                s.set_cache_tree_state(CacheTreeState::Built);
            }
        }
    }
    result
}

fn fill_gaps_body(b: &mut Brep) -> Result<(), SolidError> {
    use std::collections::BTreeSet;
    let mut k = Kernel::default();
    k.context.set_doing_boolean(true);
    let k = k;
    let vus: Vec<(VertexuseId, bool, bool)> = b
        .vertexuses()
        .iter()
        .map(|(id, r)| (id, r.vertex_face_gap.is_init(), r.vertex_edge_gap.is_init()))
        .collect();
    let edge_set: BTreeSet<VertexuseId> = vus.iter().filter(|x| x.2).map(|x| x.0).collect();
    let mut edge_done = BTreeSet::new();
    for &(vu, face_set, edge_set_here) in &vus {
        if face_set {
            fill_vertex_face_gap(&k, b, vu)?;
        }
        if edge_set_here && !edge_done.contains(&vu) {
            fill_vertex_edge_gap(&k, b, vu, &edge_set, &mut edge_done)?;
        }
    }
    let eus: Vec<(EdgeuseId, bool, bool)> = b
        .edgeuses()
        .iter()
        .map(|(id, r)| (id, r.max_edge_face_gap.is_init(), r.edge_ccw_edge_gap.is_init()))
        .collect();
    let face_set: BTreeSet<EdgeuseId> = eus.iter().filter(|x| x.1).map(|x| x.0).collect();
    let mut face_done = BTreeSet::new();
    for &(eu, face_set_here, ccw_set) in &eus {
        if face_set_here && !face_done.contains(&eu) {
            fill_edge_face_gap(&k, b, eu, &face_set, &mut face_done)?;
        }
        if ccw_set {
            fill_edge_ccw_edge_gap(&k, b, eu)?;
        }
    }
    Ok(())
}

/// `CalcVertexFaceGap` of `vu` into its own cache.
fn fill_vertex_face_gap(k: &Kernel, b: &mut Brep, vu: VertexuseId) -> Result<(), SolidError> {
    let g = vu1::calc_vertex_face_gap(k, b, vu, true, false)?.unwrap_or_default();
    b.vertexuse_mut(vu)?.vertex_face_gap = g;
    Ok(())
}

/// `CalcVertexEdgeGap` of `vu` into the set caches of it and the other uses of its
/// vertex on its edge.
fn fill_vertex_edge_gap(
    k: &Kernel,
    b: &mut Brep,
    vu: VertexuseId,
    set: &std::collections::BTreeSet<VertexuseId>,
    done: &mut std::collections::BTreeSet<VertexuseId>,
) -> Result<(), SolidError> {
    let g = vu1::calc_vertex_edge_gap(k, b, vu, true, false)?.unwrap_or_default();
    let edge = match vu1::edgeuse(b, vu)? {
        Some(eu) => b.edgeuse(eu)?.edge,
        None => None,
    };
    let siblings = match (b.vertexuse(vu)?.vertex, edge) {
        (Some(v), Some(_)) => solidcore::topology::vertex::vertexuses_of_edge(b, v, edge, None)?,
        _ => Vec::new(),
    };
    for x in std::iter::once(vu).chain(siblings) {
        if set.contains(&x) && !done.contains(&x) {
            b.vertexuse_mut(x)?.vertex_edge_gap = g;
            done.insert(x);
        }
    }
    Ok(())
}

/// `CalcMaxEdgeFaceGap` of `eu` into the set caches of it and its mate.
fn fill_edge_face_gap(
    k: &Kernel,
    b: &mut Brep,
    eu: EdgeuseId,
    set: &std::collections::BTreeSet<EdgeuseId>,
    done: &mut std::collections::BTreeSet<EdgeuseId>,
) -> Result<(), SolidError> {
    use solidcore::topology::edgeuse::p1 as eu1;
    let g = eu1::calc_max_edge_face_gap(k, b, eu, false)?;
    let mate = eu1::mate(b, eu)?;
    b.edgeuse_mut(eu)?.max_edge_face_gap = g;
    done.insert(eu);
    if let Some(m) = mate {
        if set.contains(&m) && !done.contains(&m) {
            let r = b.edgeuse_mut(m)?;
            r.max_edge_face_gap = g;
            r.max_edge_face_gap.set_edgeuse(Some(m));
            done.insert(m);
        }
    }
    Ok(())
}

/// `CalcEdgeCCWEdgeGap` of `eu` into its own cache.
fn fill_edge_ccw_edge_gap(k: &Kernel, b: &mut Brep, eu: EdgeuseId) -> Result<(), SolidError> {
    use solidcore::topology::edgeuse::p1 as eu1;
    let g = eu1::calc_edge_ccw_edge_gap(k, b, eu, true, false)?.unwrap_or_default();
    b.edgeuse_mut(eu)?.edge_ccw_edge_gap = g;
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
                    Err(e) if solidcore::error::upstream_status(&e).is_some() => {
                        // the output was reset before the failing step
                        Ok(vec![status_of(&e)?, if *want_out { 0.0 } else { -1.0 }])
                    }
                    Err(e) => Err(e),
                },
                _ => vu1::init_vertex_face_gap(b, v).map(|()| vec![0.0]),
            };
            match r {
                Ok(x) => out.extend(x),
                Err(SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }) => {
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
            let mut records = solidcore::topology::topology::NotifyRecords::new();
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
                Err(SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }) => vec![NULL_DEREF as f64],
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
                Err(SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }) => vec![NULL_DEREF as f64],
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
                Err(SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }) => vec![NULL_DEREF],
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
            Err(SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }) => vec![NULL_DEREF as f64],
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
