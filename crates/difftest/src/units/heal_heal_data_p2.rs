//! Runner for unit `heal.heal_data_p2`: `SmEdgeProps`, `SmVertexProps`, the
//! `SmProbArray` class family and the `SmHealData` target management, tolerance,
//! coincidence and degeneracy steps.
//!
//! C++: `solidcore_ffi::units::heal_heal_data_p2` (upstream `SmHealData.cpp`,
//! `SmEdgeProps.cpp`, `SmVertexProps.cpp`, `SmHealProbArray.h`). Rust:
//! `solidcore::heal::heal_data::p2`.
//!
//! Two kinds of case:
//! - `prob`: a script of `Setup` / `Add` / `Remove` / `RemoveAll` / `ReSet` /
//!   `IncOrigProbCount` / `SetLabels` / `GetAt` / member writes on one problem array
//!   (plain, pair, mixed or tried) over up to 8 standalone `SmFaceProps`; outputs each
//!   op's result, the array and every object's linked members.
//! - `heal`: an `SmHealData` built on a B-rep (an upstream `.smb` file, a wire edge, a
//!   one-face B-rep, the `my_test_topology` wire network, an `SmPrimitiveCreation`
//!   box or a full sphere face) with target lists, then a script of heal steps;
//!   outputs each step's status and results and, at the end, the whole healer state
//!   (targets, every props record ever allocated, every problem array with its labels,
//!   histograms, statistics and the vertex voxel model). The C++ side replaces analytic
//!   geometry by plain B-splines first (what the Rust import rebuilds); the Rust side
//!   imports the C++ export (the `topology.vertexuse` importer) and restores each
//!   vertex's vertex use order from the shim.
//!
//! Deferred: `Fix_TolSizes` (`SmBrep::GetThisModelSizeEstimate`, `topology.brep_p1`),
//! `Cache_DegenFaces` with a face (`SmFace::IsDegenerate` classifies its edges with
//! `SmCurveClassification`, `intersect.curve_class_p4`), and the
//! gap computations whose surface drops are not ported yet.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::context::{MarkLock, ALL_MARKS};
use solidcore::geom::vector2d::Point2;
use solidcore::heal::face_props::{
    FaceProps, HealerOpType, LoopObject, MixedLinkType, Tri, TriedLinkType,
};
use solidcore::heal::gap::{EdgeEdgeGap, EdgeFaceGap, VertexEdgeGap, VertexFaceGap};
use solidcore::heal::heal_data::p1::HealBrepTargets;
use solidcore::heal::heal_data::p2::{
    EdgeProps, EdgePropsId, FaceFlag, FaceFlagOf, FaceMixed, FaceMixedOf, FacePropsEntry,
    FacePropsId, FaceTried, FaceTriedOf, HealData, MixedProbArray, PairProbArray, ProbArray,
    PropsArena, PropsStore, TriedProbArray, VertexProps, VertexPropsId,
};
use solidcore::intersect::point_class::{ClassObject, ClassObjectKind};
use solidcore::kernel::Kernel;
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::{
    Brep, EdgeId, EdgeuseId, FaceId, LoopId, RegionId, VertexId, VertexuseId,
};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::heal_heal_data_p2 as ffi;
use solidcore_ffi::units::topology_vertexuse::counts;

use super::topology_vertexuse::{gen_brep, import, path_of, push_curve, push_surface, BrepIn};
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 600;

/// The difftest runner.
pub struct Runner;

/// The classification handles: loops, and the face, surface, topology and curves of
/// `SmFace::ClassifyLoops`' loop bisector (reached through `SetProps_Stage3`).
#[derive(Debug, Clone, Copy, PartialEq)]
enum H {
    Topo(EntityRef),
    Surface(FaceId),
    Curve(bool),
    EdgeCurve(EdgeId),
    Trim(EdgeuseId),
}

impl ClassObject for H {
    fn class_kind(&self) -> ClassObjectKind {
        match self {
            Self::Topo(e) => e.class_kind(),
            Self::Surface(_) => ClassObjectKind::Surface,
            Self::Curve(_) | Self::EdgeCurve(_) | Self::Trim(_) => ClassObjectKind::Curve,
        }
    }
}

impl LoopObject for H {
    fn from_loop(l: LoopId) -> Self {
        Self::Topo(EntityRef::Loop(l))
    }

    fn as_loop(&self) -> Option<LoopId> {
        match self {
            Self::Topo(e) => e.as_loop(),
            _ => None,
        }
    }

    fn as_vertex(&self) -> Option<VertexId> {
        match self {
            Self::Topo(e) => e.as_vertex(),
            _ => None,
        }
    }

    fn from_face(f: FaceId) -> Option<Self> {
        Some(Self::Topo(EntityRef::Face(f)))
    }

    fn from_face_surface(f: FaceId) -> Option<Self> {
        Some(Self::Surface(f))
    }

    fn from_classify_curve(uv: bool) -> Option<Self> {
        Some(Self::Curve(uv))
    }

    fn from_topology(t: EntityRef) -> Option<Self> {
        Some(Self::Topo(t))
    }

    fn as_topology(&self) -> Option<EntityRef> {
        match self {
            Self::Topo(t) => Some(*t),
            _ => None,
        }
    }

    fn from_edge_curve(e: EdgeId) -> Option<Self> {
        Some(Self::EdgeCurve(e))
    }

    fn as_edge_curve(&self) -> Option<EdgeId> {
        match self {
            Self::EdgeCurve(e) => Some(*e),
            _ => None,
        }
    }

    fn from_trim_curve(eu: EdgeuseId) -> Option<Self> {
        Some(Self::Trim(eu))
    }

    fn as_trim_curve(&self) -> Option<EdgeuseId> {
        match self {
            Self::Trim(eu) => Some(*eu),
            _ => None,
        }
    }
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// "Upstream would read freed or null memory or past an array" (the shim refused).
const NULL_DEREF: f64 = -2.0;
/// No alive props to select.
const NONE_ALIVE: f64 = -3.0;
/// Start of the final state.
const STATE: f64 = -100.0;

// ---- inputs -----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Prim {
    /// The single-vertex sphere regression of `smTopoTest.cpp`.
    SingleVertexSphere {
        radius: f64,
        #[serde(default)]
        swap: bool,
    },
    /// `SmPrimitiveCreation::CreateBox(dx, dy, dz, SmAxis2Placement())`.
    Box { size: [f64; 3] },
    /// A face on the full domain of `SmSphere::CreateCanonical(radius)`.
    Sphere {
        radius: f64,
        #[serde(default)]
        swap: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum BIn {
    Prim(Prim),
    Std(BrepIn),
}

/// A target list: every entity of the kind in order, in reverse order, or explicit
/// selectors (taken modulo the count).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Sel {
    All,
    Reverse,
    None,
    List(Vec<u64>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Targets {
    pub(crate) v: Sel,
    pub(crate) e: Sel,
    pub(crate) f: Sel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub(crate) enum Step {
    CacheEdgeProps,
    CacheVertexProps,
    CacheFacePropsGaps,
    FixTolSizes,
    CacheCoinVertices,
    CacheDegenFaces,
    CacheCoinEdges,
    FixCoinEdges,
    CacheMissedEdgeXsects,
    FixMissedEdgeXsects,
    FixUncontainedEdges,
    FixBadGaps,
    RemoveVertexProps {
        sel: u64,
    },
    RemoveEdgeProps {
        sel: u64,
    },
    RemoveFaceProps {
        sel: u64,
    },
    /// `which`: 0 vertex, 1 edge, 2 face props; `tgts`: `[kind, index]` with `kind`
    /// the export's entity kinds (0 region .. 9 vertex).
    PropsForTgts {
        which: u8,
        stale: bool,
        tgts: Vec<[u64; 2]>,
    },
    FreeTargets,
    LoadTargets {
        targets: Targets,
    },
    SetOldCounts {
        counts: [u64; 3],
    },
    SetDoneHealOp {
        op: u8,
    },
    ResetEdgeProps {
        sel: u64,
        metadata: bool,
    },
    ResetVertexProps {
        sel: u64,
    },
    SetupEmptyProbArrays,
    // ---- steps of unit heal.heal_data_p1 (runner `heal_heal_data_p1`) ----
    /// `ReSet(new_obj_arrays)` (last step only).
    Reset {
        new_obj_arrays: bool,
    },
    /// `MoveToNotYetProblems` of the `sel`-th alive face props.
    MoveToNotYet {
        sel: u64,
    },
    CacheFacePropsStage2,
    CacheFacePropsStage3,
    FixBadSheets,
    MakeUvTrimCurves {
        make: bool,
    },
    /// Last step only.
    FixInfiniteRegion,
    /// `HealBrep`: `mode` 0 null B-rep, 1 every target, 2 `targets`; modes 1 and 2
    /// last step only.
    HealBrep {
        mode: u8,
        op: u8,
        make: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        targets: Option<Targets>,
    },
    /// The stage 2/3 snapshot of every face props record.
    FaceSnapshot,
    // ---- heal.heal_data_p3 ----
    HealTargets {
        op: u8,
        make: bool,
    },
    UpdateTargets {
        op: u8,
        remove: Vec<[u64; 2]>,
        add: Vec<[u64; 2]>,
    },
    FixCoinVertices,
    SeedDegenFaces,
    SeedFaceProblems {
        null_face: bool,
    },
    FixDegenFaces,
    FixDegenEdges,
    FixMoveSeam,
    FixSplitEdgesAtSeam,
    FixSplitFaceAtSeams,
    UpdateTracked {
        active: bool,
        op: u8,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "case", rename_all = "snake_case")]
pub(crate) enum Inputs {
    Prob {
        /// 0 plain, 1 pair, 2 mixed, 3 tried.
        kind: u8,
        n: u64,
        /// `[code, a, b, c]` (see `solidcore_ffi::units::heal_heal_data_p2::check_prob`).
        ops: Vec<[i64; 4]>,
    },
    Heal {
        brep: Box<BIn>,
        #[serde(default)]
        validate_only: bool,
        /// Compute the cached gaps right after building (C++; the Rust side imports
        /// them).
        #[serde(default)]
        init_gaps: bool,
        targets: Targets,
        #[serde(default = "default_voxels")]
        voxels: [u64; 3],
        steps: Vec<Step>,
    },
}

fn default_voxels() -> [u64; 3] {
    [12, 12, 12]
}

// ---- encoding ---------------------------------------------------------------------------

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

fn block_of(
    b: &BIn,
    validate_only: bool,
    init_gaps: bool,
) -> Result<(String, Vec<f64>), HarnessError> {
    let mut v = Vec::new();
    let path = match b {
        BIn::Prim(Prim::Box { size }) => {
            v.push(4.0);
            v.extend(size);
            String::new()
        }
        BIn::Prim(Prim::SingleVertexSphere { radius, swap }) => {
            v.extend([6.0, *radius, flag(*swap)]);
            String::new()
        }
        BIn::Prim(Prim::Sphere { radius, swap }) => {
            v.extend([5.0, *radius, flag(*swap)]);
            String::new()
        }
        BIn::Std(s) => {
            match s {
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
            path_of(s)?
        }
    };
    v.push(flag(validate_only));
    v.push(flag(init_gaps));
    Ok((path, v))
}

fn resolve(s: &Sel, n: usize) -> Vec<u64> {
    match s {
        Sel::All => (0..n as u64).collect(),
        Sel::Reverse => (0..n as u64).rev().collect(),
        Sel::None => Vec::new(),
        Sel::List(l) => {
            if n == 0 {
                Vec::new()
            } else {
                l.iter().map(|x| x % n as u64).collect()
            }
        }
    }
}

/// `[n_v, n_e, n_f]` of an export.
fn vef(c: [usize; 10]) -> [usize; 3] {
    [c[9], c[7], c[3]]
}

fn push_targets(v: &mut Vec<f64>, t: &Targets, n: [usize; 3]) {
    for (s, k) in [(&t.v, n[0]), (&t.e, n[1]), (&t.f, n[2])] {
        let l = resolve(s, k);
        v.push(l.len() as f64);
        v.extend(l.iter().map(|&x| x as f64));
    }
}

fn args_of(t: &Targets, voxels: [u64; 3], steps: &[Step], n: [usize; 3]) -> Vec<f64> {
    let mut v = Vec::new();
    push_targets(&mut v, t, n);
    v.extend(voxels.iter().map(|&x| x as f64));
    v.push(steps.len() as f64);
    for s in steps {
        match s {
            Step::CacheEdgeProps => v.push(0.0),
            Step::CacheVertexProps => v.push(1.0),
            Step::CacheFacePropsGaps => v.push(2.0),
            Step::FixTolSizes => v.push(3.0),
            Step::CacheCoinVertices => v.push(4.0),
            Step::CacheDegenFaces => v.push(5.0),
            Step::CacheCoinEdges => v.push(6.0),
            Step::FixCoinEdges => v.push(7.0),
            Step::CacheMissedEdgeXsects => v.push(8.0),
            Step::FixMissedEdgeXsects => v.push(9.0),
            Step::FixUncontainedEdges => v.push(10.0),
            Step::FixBadGaps => v.push(11.0),
            Step::RemoveVertexProps { sel } => v.extend([12.0, *sel as f64]),
            Step::RemoveEdgeProps { sel } => v.extend([13.0, *sel as f64]),
            Step::RemoveFaceProps { sel } => v.extend([14.0, *sel as f64]),
            Step::PropsForTgts { which, stale, tgts } => {
                v.extend([15.0, f64::from(*which), flag(*stale), tgts.len() as f64]);
                for [k, i] in tgts {
                    v.extend([*k as f64, *i as f64]);
                }
            }
            Step::FreeTargets => v.push(16.0),
            Step::LoadTargets { targets } => {
                v.push(17.0);
                push_targets(&mut v, targets, n);
            }
            Step::SetOldCounts { counts } => {
                v.push(18.0);
                v.extend(counts.iter().map(|&x| x as f64));
            }
            Step::SetDoneHealOp { op } => v.extend([19.0, f64::from(*op)]),
            Step::ResetEdgeProps { sel, metadata } => {
                v.extend([20.0, *sel as f64, flag(*metadata)]);
            }
            Step::ResetVertexProps { sel } => v.extend([21.0, *sel as f64]),
            Step::SetupEmptyProbArrays => v.push(22.0),
            Step::Reset { new_obj_arrays } => v.extend([23.0, flag(*new_obj_arrays)]),
            Step::MoveToNotYet { sel } => v.extend([24.0, *sel as f64]),
            Step::CacheFacePropsStage2 => v.push(25.0),
            Step::CacheFacePropsStage3 => v.push(26.0),
            Step::FixBadSheets => v.push(27.0),
            Step::MakeUvTrimCurves { make } => v.extend([28.0, flag(*make)]),
            Step::FixInfiniteRegion => v.push(29.0),
            Step::HealBrep {
                mode,
                op,
                make,
                targets,
            } => {
                v.extend([30.0, f64::from(*mode), f64::from(*op), flag(*make)]);
                if *mode == 2 {
                    let none = Targets {
                        v: Sel::None,
                        e: Sel::None,
                        f: Sel::None,
                    };
                    push_targets(&mut v, targets.as_ref().unwrap_or(&none), n);
                }
            }
            Step::FaceSnapshot => v.push(31.0),
            Step::HealTargets { op, make } => v.extend([32.0, f64::from(*op), flag(*make)]),
            Step::UpdateTargets { op, remove, add } => {
                v.extend([33.0, f64::from(*op), remove.len() as f64]);
                for [kind, sel] in remove {
                    v.extend([*kind as f64, *sel as f64]);
                }
                v.push(add.len() as f64);
                for [kind, sel] in add {
                    v.extend([*kind as f64, *sel as f64]);
                }
            }
            Step::FixCoinVertices => v.push(34.0),
            Step::SeedDegenFaces => v.push(41.0),
            Step::SeedFaceProblems { null_face } => v.extend([42.0, flag(*null_face)]),
            Step::FixDegenFaces => v.push(35.0),
            Step::FixDegenEdges => v.push(36.0),
            Step::FixMoveSeam => v.push(37.0),
            Step::FixSplitEdgesAtSeam => v.push(38.0),
            Step::FixSplitFaceAtSeams => v.push(39.0),
            Step::UpdateTracked { active, op } => v.extend([40.0, flag(*active), f64::from(*op)]),
        }
    }
    v
}

fn prob_input(kind: u8, n: u64, ops: &[[i64; 4]]) -> Vec<f64> {
    let mut v = vec![f64::from(kind), n as f64, ops.len() as f64];
    for op in ops {
        v.extend(op.iter().map(|&x| x as f64));
    }
    v
}

// ---- C++ --------------------------------------------------------------------------------

fn ffi_out(r: Result<Vec<f64>, solidcore_ffi::FfiError>) -> Result<Output, HarnessError> {
    match r {
        Ok(v) => Ok(Output::new().values("out", v)),
        Err(solidcore_ffi::FfiError::Status(s)) => Ok(Output::err(format!("status_{s}"))),
        Err(e) => Err(harness(format!("c++: {e}"))),
    }
}

/// The export of a heal case's B-rep and its `[n_v, n_e, n_f]` (`None` when it could
/// not be built).
fn export_of(path: &str, block: &[f64]) -> Result<(Vec<f64>, Option<[usize; 3]>), HarnessError> {
    let ex = ffi::export(path, block).map_err(|e| harness(format!("c++ export: {e}")))?;
    let n = counts(&ex).map(vef);
    Ok((ex, n))
}

pub(crate) fn run_cpp(i: &Inputs) -> Result<Output, HarnessError> {
    match i {
        Inputs::Prob { kind, n, ops } => ffi_out(ffi::prob(&prob_input(*kind, *n, ops))),
        Inputs::Heal {
            brep,
            validate_only,
            init_gaps,
            targets,
            voxels,
            steps,
        } => {
            let (path, block) = block_of(brep, *validate_only, *init_gaps)?;
            let (_, n) = export_of(&path, &block)?;
            let n = n.unwrap_or([0; 3]);
            let args = args_of(targets, *voxels, steps, n);
            if steps.iter().any(|s| {
                matches!(
                    s,
                    Step::HealTargets { .. }
                        | Step::UpdateTargets { .. }
                        | Step::FixCoinVertices
                        | Step::FixDegenFaces
                        | Step::FixDegenEdges
                        | Step::FixMoveSeam
                        | Step::FixSplitEdgesAtSeam
                        | Step::FixSplitFaceAtSeams
                        | Step::UpdateTracked { .. }
                )
            }) {
                ffi_out(solidcore_ffi::units::heal_heal_data_p3::heal(
                    &path, &block, &args,
                ))
            } else {
                ffi_out(ffi::heal(&path, &block, &args))
            }
        }
    }
}

// ---- Rust: problem arrays -------------------------------------------------------------

fn tried_of(c: i64) -> TriedLinkType {
    match c {
        1 => TriedLinkType::Ran,
        2 => TriedLinkType::Fixed,
        3 => TriedLinkType::NoFix,
        _ => TriedLinkType::None,
    }
}

const LABELS: [[&str; 3]; 3] = [
    [
        "Edge ZoneTol3d too small                ",
        "SmallTol Edge Cnt  ",
        " - Bad ",
    ],
    [
        "Sheet Face counts                       ",
        "                   ",
        "",
    ],
    [
        "odd NoAreaLoopCnt on Face: no AddSeamFix",
        "Bad unpaired NoAreaLoop Face Cnt ",
        " - Warn ",
    ],
];

fn put_str(o: &mut Vec<f64>, s: &str) {
    o.push(s.len() as f64);
    o.extend(s.bytes().map(f64::from));
}

fn put_base<I: Copy, L>(o: &mut Vec<f64>, a: &ProbArray<I, L>, idx: impl Fn(I) -> f64) {
    o.push(a.prob_array.len() as f64);
    o.extend(a.prob_array.iter().map(|&e| idx(e)));
    o.push(a.orig_prob_count as f64);
    o.push(a.get_healer_op.code() as f64);
    put_str(o, &a.output_label);
    put_str(o, &a.data_label);
    put_str(o, &a.prob_label);
}

fn put_face_flags(o: &mut Vec<f64>, p: &FaceProps<H>) {
    for f in FaceFlag::ALL {
        o.push(f.value(p) as f64);
    }
    o.push(FaceMixed::Sheet.value(p).code() as f64);
    o.push(FaceMixed::Poles.value(p).code() as f64);
    for t in [
        FaceTried::MoveSeam,
        FaceTried::SplitEdgesAtSeam,
        FaceTried::SplitFacesAtSeam,
    ] {
        o.push(t.value(p).code() as f64);
    }
}

fn op_of(c: i64) -> HealerOpType {
    HealerOpType::from_code(c).unwrap_or(HealerOpType::None)
}

#[allow(clippy::too_many_lines)]
fn rust_prob(kind: u8, n: u64, ops: &[[i64; 4]]) -> Result<Output, HarnessError> {
    // Run only on scripts the C++ guard accepts: `n` sizes the object list below.
    ffi::check_prob(&prob_input(kind, n, ops)).map_err(|e| harness(format!("prob script: {e}")))?;
    let mut arena: PropsArena<FacePropsId, FacePropsEntry<H>> = PropsArena::default();
    let ids: Vec<FacePropsId> = (0..n).map(|_| arena.alloc(FacePropsEntry::new())).collect();
    let id = |a: i64| -> Result<FacePropsId, HarnessError> {
        usize::try_from(a)
            .ok()
            .and_then(|a| ids.get(a).copied())
            .ok_or_else(|| harness("object index"))
    };
    let mut plain: ProbArray<FacePropsId, FaceFlagOf<H>> = ProbArray::default();
    let mut pair: PairProbArray<FacePropsId, FaceFlagOf<H>> = PairProbArray::default();
    let mut mixed: MixedProbArray<FacePropsId, FaceMixedOf<H>> = MixedProbArray::default();
    let mut tried: TriedProbArray<FacePropsId, FaceTriedOf<H>> = TriedProbArray::default();
    let link = |b: i64| -> Option<FaceFlagOf<H>> {
        usize::try_from(b)
            .ok()
            .and_then(|b| FaceFlag::ALL.get(b).copied())
            .map(FaceFlagOf::new)
    };
    let mut o = Vec::new();
    let mut stop = false;
    let e2s = |e: SolidError| e.to_string();
    for &[code, a, b, c] in ops {
        if stop {
            continue;
        }
        o.push(code as f64);
        match code {
            0 => {
                match kind {
                    2 => mixed.setup(
                        op_of(a),
                        match b {
                            0 => Some(FaceMixedOf::new(FaceMixed::Sheet)),
                            1 => Some(FaceMixedOf::new(FaceMixed::Poles)),
                            _ => None,
                        },
                    ),
                    3 => tried.setup(
                        op_of(a),
                        match b {
                            0 => Some(FaceTriedOf::new(FaceTried::MoveSeam)),
                            1 => Some(FaceTriedOf::new(FaceTried::SplitEdgesAtSeam)),
                            2 => Some(FaceTriedOf::new(FaceTried::SplitFacesAtSeam)),
                            _ => None,
                        },
                    ),
                    1 => pair.base.setup(op_of(a), link(b)),
                    _ => plain.setup(op_of(a), link(b)),
                }
                o.push(0.0);
            }
            1 => {
                let p = id(a)?;
                let r = match kind {
                    2 => mixed.add(
                        &mut arena,
                        p,
                        MixedLinkType::from_code(b).unwrap_or(MixedLinkType::Undef),
                    ),
                    3 => tried.add(&mut arena, p, tried_of(b)),
                    1 => pair.base.add(&mut arena, p, b != 0),
                    _ => plain.add(&mut arena, p, b != 0),
                };
                o.push(r.map_err(|e| harness(e2s(e)))? as f64);
            }
            2 => {
                let p = id(a)?;
                let r = match kind {
                    1 => match pair.remove(&arena, p) {
                        Ok(r) => r,
                        Err(SolidError::InvalidInput { .. }) => {
                            o.push(NULL_DEREF);
                            stop = true;
                            continue;
                        }
                        Err(e) => return Err(harness(e2s(e))),
                    },
                    2 => mixed.base.remove(p),
                    3 => tried.base.remove(p),
                    _ => plain.remove(p),
                };
                o.push(flag(r));
            }
            3 => {
                match kind {
                    1 => pair.base.remove_all(),
                    2 => mixed.base.remove_all(),
                    3 => tried.base.remove_all(),
                    _ => plain.remove_all(),
                }
                o.push(0.0);
            }
            4 => {
                match kind {
                    1 => pair.reset(),
                    2 => mixed.reset(),
                    3 => tried.reset(),
                    _ => plain.reset(),
                }
                o.push(0.0);
            }
            5 => {
                let r = match kind {
                    1 => pair.base.inc_orig_prob_count(),
                    2 => mixed.base.inc_orig_prob_count(),
                    3 => tried.base.inc_orig_prob_count(),
                    _ => plain.inc_orig_prob_count(),
                };
                o.push(r as f64);
            }
            6 => {
                let p = id(a)?;
                let f = usize::try_from(b)
                    .ok()
                    .and_then(|b| FaceFlag::ALL.get(b).copied())
                    .ok_or_else(|| harness("flag"))?;
                let e = arena.props_mut(p).map_err(|e| harness(e2s(e)))?;
                f.assign(&mut e.props, c as u64);
                o.push(0.0);
            }
            7 => {
                let l = usize::try_from(a)
                    .ok()
                    .and_then(|a| LABELS.get(a))
                    .ok_or_else(|| harness("labels"))?;
                match kind {
                    1 => pair.base.set_labels(l[0], l[1], l[2]),
                    2 => mixed.base.set_labels(l[0], l[1], l[2]),
                    3 => tried.base.set_labels(l[0], l[1], l[2]),
                    _ => plain.set_labels(l[0], l[1], l[2]),
                }
                o.push(0.0);
            }
            8 => {
                let arr = match kind {
                    1 => &pair.base.prob_array,
                    2 => &mixed.base.prob_array,
                    3 => &tried.base.prob_array,
                    _ => &plain.prob_array,
                };
                if arr.is_empty() {
                    o.push(-1.0);
                } else {
                    let k = (a as u64 % arr.len() as u64) as usize;
                    o.push(arr.get(k).map_or(-3.0, |e| e.index() as f64));
                }
            }
            9 => {
                match kind {
                    1 => pair.set_orig_coin_pair_cnt(a as u64),
                    2 => mixed.base.set_orig_prob_cnt(a as u64),
                    3 => tried.base.set_orig_prob_cnt(a as u64),
                    _ => plain.set_orig_prob_cnt(a as u64),
                }
                o.push(0.0);
            }
            _ => return Err(harness("op code")),
        }
    }
    if !stop {
        o.push(STATE);
        let idx = |e: FacePropsId| e.index() as f64;
        let orig = match kind {
            1 => {
                put_base(&mut o, &pair.base, idx);
                o.push(pair.orig_coin_pair_cnt as f64);
                pair.base.orig_prob_count
            }
            2 => {
                put_base(&mut o, &mixed.base, idx);
                o.push(0.0);
                mixed.base.orig_prob_count
            }
            3 => {
                put_base(&mut o, &tried.base, idx);
                o.push(0.0);
                tried.base.orig_prob_count
            }
            _ => {
                put_base(&mut o, &plain, idx);
                o.push(0.0);
                plain.orig_prob_count
            }
        };
        o.push(orig as f64);
        for &p in &ids {
            let e = arena.props(p).map_err(|e| harness(e2s(e)))?;
            put_face_flags(&mut o, &e.props);
        }
    }
    Ok(Output::new().values("out", o))
}

// ---- Rust: the healer -----------------------------------------------------------------

/// How a step's Rust failure is reported.
enum Fail {
    /// The case stops with `-2` (upstream undefined behaviour, refused by the shim).
    NullDeref,
    /// An upstream status: `status new_probs`, then the case stops.
    Status(i32),
    /// Unported callee: the case is deferred.
    Deferred(Output),
    /// Anything else (a bug).
    Other(String),
}

fn classify(e: SolidError) -> Fail {
    if let Some(o) = Output::unported(&e) {
        return Fail::Deferred(o);
    }
    match e {
        SolidError::InvalidInput { .. } => Fail::NullDeref,
        SolidError::Upstream { code, .. } => Fail::Status(code),
        e => Fail::Other(e.to_string()),
    }
}

fn ix(i: Option<u32>) -> f64 {
    i.map_or(-1.0, f64::from)
}

fn pick<I: Copy>(alive: &[I], sel: u64) -> Option<I> {
    if alive.is_empty() {
        None
    } else {
        alive.get((sel % alive.len() as u64) as usize).copied()
    }
}

fn target_count_after_removals<T: Copy + PartialEq>(
    targets: &[T],
    removed: impl Iterator<Item = T>,
) -> usize {
    let mut remaining = targets.to_vec();
    for t in removed {
        if let Some(i) = remaining.iter().position(|&x| x == t) {
            remaining.remove(i);
        }
    }
    remaining.len()
}

fn removal_lookup_is_ub<T: PartialEq>(targets: &[T], props_count: usize, removed: &T) -> bool {
    targets
        .iter()
        .position(|x| x == removed)
        .is_some_and(|i| i >= props_count)
}

fn targets_of(t: &Targets, n: [usize; 3]) -> (Vec<VertexId>, Vec<EdgeId>, Vec<FaceId>) {
    let v = resolve(&t.v, n[0])
        .into_iter()
        .map(|x| VertexId::from_index(x as u32))
        .collect();
    let e = resolve(&t.e, n[1])
        .into_iter()
        .map(|x| EdgeId::from_index(x as u32))
        .collect();
    let f = resolve(&t.f, n[2])
        .into_iter()
        .map(|x| FaceId::from_index(x as u32))
        .collect();
    (v, e, f)
}

fn entity(kind: u64, index: u64, c: [usize; 10]) -> Option<EntityRef> {
    use solidcore::topology::{FaceuseId, LoopuseId, RegionId, ShellId};
    let n = *c.get(kind as usize)?;
    if n == 0 {
        return None;
    }
    let i = (index % n as u64) as u32;
    Some(match kind {
        0 => EntityRef::Region(RegionId::from_index(i)),
        1 => EntityRef::Shell(ShellId::from_index(i)),
        2 => EntityRef::Faceuse(FaceuseId::from_index(i)),
        3 => EntityRef::Face(FaceId::from_index(i)),
        4 => EntityRef::Loopuse(LoopuseId::from_index(i)),
        5 => EntityRef::Loop(LoopId::from_index(i)),
        6 => EntityRef::Edgeuse(EdgeuseId::from_index(i)),
        7 => EntityRef::Edge(EdgeId::from_index(i)),
        8 => EntityRef::Vertexuse(VertexuseId::from_index(i)),
        _ => EntityRef::Vertex(VertexId::from_index(i)),
    })
}

/// The healer state, as the shim's `dump`.
#[allow(clippy::too_many_lines)]
fn dump(h: &HealData<H>, b: &Brep, o: &mut Vec<f64>) -> Result<(), SolidError> {
    o.push(STATE);
    o.push(h.tgt_vertices.len() as f64);
    o.extend(h.tgt_vertices.iter().map(|v| f64::from(v.index())));
    o.push(h.tgt_edges.len() as f64);
    o.extend(h.tgt_edges.iter().map(|v| f64::from(v.index())));
    o.push(h.tgt_faces.len() as f64);
    o.extend(h.tgt_faces.iter().map(|v| f64::from(v.index())));
    o.push(h.tgt_vertex_props.len() as f64);
    o.extend(h.tgt_vertex_props.iter().map(|p| p.index() as f64));
    o.push(h.tgt_edge_props.len() as f64);
    o.extend(h.tgt_edge_props.iter().map(|p| p.index() as f64));
    o.push(h.tgt_face_props.len() as f64);
    o.extend(h.tgt_face_props.iter().map(|p| p.index() as f64));
    o.extend([
        h.old_vertex_cnt as f64,
        h.old_edge_cnt as f64,
        h.old_face_cnt as f64,
        h.stop_heal_op.code() as f64,
        h.done_heal_op.code() as f64,
        h.recursion_count as f64,
    ]);

    o.push(h.vertex_props.allocated() as f64);
    for k in 0..h.vertex_props.allocated() {
        let Some(p) = h.vertex_props.get(VertexPropsId(k)) else {
            o.push(0.0);
            continue;
        };
        let p: &VertexProps = p;
        o.push(1.0);
        o.push(ix(p.vertex.map(VertexId::index)));
        o.push(p.vertex_index as f64);
        o.push(p.zone_tol_3d);
        o.push(p.orig_zone_tol_3d);
        o.push(ix(p.max_gap_3d_vtx_edge.map(VertexuseId::index)));
        o.push(p.max_vtx_edge_gap_length(b)?);
        o.push(ix(p.max_gap_3d_vtx_face.map(VertexuseId::index)));
        o.push(p.max_vtx_face_gap_length(b)?);
        o.extend([
            p.bad_small_zone_tol_3d.code() as f64,
            p.bad_large_zone_tol_3d.code() as f64,
            p.warn_large_gap_3d.code() as f64,
            p.bad_coincident_vertex.code() as f64,
            p.base.bad_back_pointer.code() as f64,
            flag(p.is_set()),
            flag(p.has_problems()),
        ]);
    }
    o.push(h.edge_props.allocated() as f64);
    for k in 0..h.edge_props.allocated() {
        let Some(p) = h.edge_props.get(EdgePropsId(k)) else {
            o.push(0.0);
            continue;
        };
        let p: &EdgeProps = p;
        o.push(1.0);
        o.push(ix(p.edge.map(EdgeId::index)));
        o.push(p.edge_index as f64);
        o.extend([p.edge_interval.min(), p.edge_interval.max()]);
        o.push(p.zone_tol_3d);
        o.push(p.orig_zone_tol_3d);
        o.push(p.approx_edge_length_3d);
        o.push(flag(p.has_curve));
        o.extend([p.natural_interval.min(), p.natural_interval.max()]);
        o.push(ix(p.max_gap_3d_edge_edge.map(EdgeuseId::index)));
        o.push(p.max_edge_edge_gap_length(b)?);
        o.push(ix(p.max_gap_3d_edge_face.map(EdgeuseId::index)));
        o.push(p.max_edge_face_gap_length(b)?);
        o.extend([
            p.bad_small_zone_tol_3d.code() as f64,
            p.bad_large_zone_tol_3d.code() as f64,
            p.warn_large_gap_3d.code() as f64,
            p.bad_degen_edge.code() as f64,
            p.bad_uncontained_edge.code() as f64,
            p.bad_coincident_edge.code() as f64,
            p.bad_missed_edge_xsect.code() as f64,
            p.base.bad_back_pointer.code() as f64,
            flag(p.is_set_metadata()),
            flag(p.is_set_gaps()),
            flag(p.has_problems()),
        ]);
    }
    o.push(h.face_props.allocated() as f64);
    for k in 0..h.face_props.allocated() {
        let Some(e) = h.face_props.get(FacePropsId(k)) else {
            o.push(0.0);
            continue;
        };
        let p = &e.props;
        o.push(1.0);
        o.push(ix(p.face.map(FaceId::index)));
        o.push(p.face_index as f64);
        o.push(p.zone_tol_3d);
        o.push(p.orig_zone_tol_3d);
        o.push(p.prop_stage.code() as f64);
        o.push(e.base.bad_back_pointer.code() as f64);
        put_face_flags(o, p);
    }

    let vi = |e: VertexPropsId| e.index() as f64;
    let ei = |e: EdgePropsId| e.index() as f64;
    let fi = |e: FacePropsId| e.index() as f64;
    put_base(o, &h.not_yet_face_props, fi);
    {
        let a = &h.bad_obj_props_back_pointers;
        o.push(a.prob_array.len() as f64);
        o.push(a.orig_prob_count as f64);
        o.push(a.get_healer_op.code() as f64);
        put_str(o, &a.output_label);
        put_str(o, &a.data_label);
        put_str(o, &a.prob_label);
    }
    put_base(o, &h.bad_edge_props_uncontained, ei);
    put_base(o, &h.bad_edge_props_small_zone_tol_3d, ei);
    put_base(o, &h.bad_edge_props_large_zone_tol_3d, ei);
    put_base(o, &h.warn_edge_props_large_gap_3d, ei);
    put_base(o, &h.bad_edge_props_degen_edges, ei);
    put_base(o, &h.bad_vertex_props_small_zone_tol_3d, vi);
    put_base(o, &h.bad_vertex_props_large_zone_tol_3d, vi);
    put_base(o, &h.warn_vertex_props_large_gap_3d, vi);
    put_base(o, &h.bad_face_props_small_zone_tol_3d, fi);
    put_base(o, &h.bad_face_props_large_zone_tol_3d, fi);
    put_base(o, &h.bad_vertex_props_coin_vertices.base, vi);
    o.push(h.bad_vertex_props_coin_vertices.orig_coin_pair_cnt as f64);
    put_base(o, &h.bad_face_props_degen_faces, fi);
    put_base(o, &h.bad_edge_props_coin_edges.base, ei);
    o.push(h.bad_edge_props_coin_edges.orig_coin_pair_cnt as f64);
    put_base(o, &h.bad_edge_props_missed_edge_xsects.base, ei);
    o.push(h.bad_edge_props_missed_edge_xsects.orig_coin_pair_cnt as f64);
    put_base(o, &h.face_props_sheets.base, fi);
    put_base(o, &h.face_props_no_vertex_pole.base, fi);
    put_base(o, &h.bad_face_props_flat_corner, fi);
    put_base(o, &h.bad_face_props_missing_seam, fi);
    put_base(o, &h.bad_face_props_crossed_seam, fi);
    put_base(o, &h.bad_face_props_near_miss_seam, fi);
    put_base(o, &h.ran_face_props_move_seam.base, fi);
    put_base(o, &h.ran_face_props_split_edges_at_seam.base, fi);
    put_base(o, &h.bad_face_props_loop_problems, fi);
    put_base(o, &h.bad_face_props_outer_loop_order, fi);
    put_base(o, &h.bad_face_props_orient_loop, fi);
    put_base(o, &h.bad_face_props_no_area_loop, fi);
    put_base(o, &h.bad_face_props_multi_outer_loops, fi);
    put_base(o, &h.bad_face_props_nested_outer_loops, fi);
    put_base(o, &h.bad_face_props_unpaired_no_area_loop, fi);
    put_base(o, &h.bad_face_props_closed_3d_loop, fi);
    put_base(o, &h.bad_face_props_closed_ptr_loop, fi);
    put_base(o, &h.bad_face_props_missing_pole, fi);
    put_base(o, &h.ran_face_props_split_face_at_seam.base, fi);
    o.push(h.ran_region_test_closure_cnt as f64);
    o.push(h.fix_region_split_region_cnt as f64);

    for a in [
        &h.loop_gap_3d_histogram,
        &h.edge_face_gap_3d_histogram,
        &h.edge_len_3d_histogram,
        &h.vert_edge_gap_3d_histogram,
        &h.vert_face_gap_3d_histogram,
        &h.vert_vert_gap_3d_histogram,
    ] {
        o.push(a.len() as f64);
        o.extend(a.iter().map(|&x| x as f64));
    }
    o.extend([
        h.max_edge_length,
        h.avg_edge_length,
        h.non_degen_min_edge_length,
        h.non_degen_edge_cnt as f64,
    ]);

    o.extend([h.x_cnt as f64, h.y_cnt as f64, h.z_cnt as f64]);
    let (mn, mx) = (h.vertex_bbox.min(), h.vertex_bbox.max());
    o.extend([mn.x, mn.y, mn.z, mx.x, mx.y, mx.z]);
    let items = h.vertices_in_voxels.point_items();
    o.push(items.len() as f64);
    for it in items {
        o.push(it.item.index() as f64);
        o.extend([it.point.x, it.point.y, it.point.z, it.zone_tol]);
    }
    Ok(())
}

/// Restores what the import cannot carry: each vertex's vertex use order and the
/// cached gap records of every edge use and vertex use (see the shim's
/// `put_cached_gaps`).
fn restore(b: &mut Brep, extra: &[f64], c: [usize; 10]) -> Result<(), HarnessError> {
    let mut at = 1usize;
    let mut next = || -> Result<f64, HarnessError> {
        let x = *extra.get(at).ok_or_else(|| harness("extras too short"))?;
        at += 1;
        Ok(x)
    };
    let he = |e: SolidError| harness(e.to_string());
    for v in 0..c[9] {
        let len = next()? as usize;
        let mut l = Vec::new();
        for _ in 0..len {
            let x = next()?;
            if x < 0.0 {
                return Err(harness("unregistered vertex use in a vertex list"));
            }
            l.push(VertexuseId::from_index(x as u32));
        }
        b.vertex_mut(VertexId::from_index(v as u32))
            .map_err(he)?
            .vertexuses = l;
    }
    let eu_of = |x: f64| -> Result<Option<EdgeuseId>, HarnessError> {
        match x {
            -1.0 => Ok(None),
            x if x >= 0.0 => Ok(Some(EdgeuseId::from_index(x as u32))),
            _ => Err(harness("unregistered edge use in a gap")),
        }
    };
    let vu_of = |x: f64| -> Result<Option<VertexuseId>, HarnessError> {
        match x {
            -1.0 => Ok(None),
            x if x >= 0.0 => Ok(Some(VertexuseId::from_index(x as u32))),
            _ => Err(harness("unregistered vertex use in a gap")),
        }
    };
    for e in 0..c[6] {
        let _init = next()?;
        let ccw = EdgeEdgeGap {
            gap_3d: next()?,
            edgeuse0: eu_of(next()?)?,
            edgeuse1: eu_of(next()?)?,
            edge_t0: next()?,
            edge_t1: next()?,
        };
        let _init = next()?;
        let face = EdgeFaceGap {
            gap_3d: next()?,
            edgeuse: eu_of(next()?)?,
            edge_t: next()?,
            face_uv: Point2::new(next()?, next()?),
        };
        let r = b.edgeuse_mut(EdgeuseId::from_index(e as u32)).map_err(he)?;
        r.edge_ccw_edge_gap = ccw;
        r.max_edge_face_gap = face;
    }
    for u in 0..c[8] {
        let _init = next()?;
        let eg = VertexEdgeGap {
            gap_3d: next()?,
            vertexuse: vu_of(next()?)?,
            edge_t: next()?,
        };
        let _init = next()?;
        let fg = VertexFaceGap {
            gap_3d: next()?,
            vertexuse: vu_of(next()?)?,
            face_uv: Point2::new(next()?, next()?),
        };
        let r = b
            .vertexuse_mut(VertexuseId::from_index(u as u32))
            .map_err(he)?;
        r.vertex_edge_gap = eg;
        r.vertex_face_gap = fg;
    }
    // the infinite region (`SmBrep::m_pInfiniteRegion`; `Fix_InfiniteRegion` walks the
    // region list from it)
    let inf = next()?;
    if inf >= 0.0 {
        b.set_infinite_region(Some(RegionId::from_index(inf as u32)));
    }
    if at != extra.len() {
        return Err(harness("extras: trailing data"));
    }
    Ok(())
}

/// The stage 2/3 values of a face props record, as the shim's `put_face_snapshot`.
fn put_face_snapshot(o: &mut Vec<f64>, p: &FaceProps<H>) {
    let t = |x: Tri| x.code() as f64;
    o.push(ix(p.face.map(FaceId::index)));
    o.push(p.face_index as f64);
    o.push(p.prop_stage.code() as f64);
    o.push(p.not_yet_face_type.code() as f64);
    o.push(flag(p.has_surface));
    let d = &p.natural_uv_domain;
    o.extend([d.u_min(), d.v_min(), d.u_max(), d.v_max()]);
    o.extend([t(p.closed_u), t(p.closed_v), t(p.closed_surf)]);
    o.push(f64::from(p.closed_val_u as i32));
    o.push(f64::from(p.closed_val_v as i32));
    o.extend([
        p.poles as f64,
        p.approx_poles as f64,
        p.no_vertex_poles as f64,
    ]);
    for l in [&p.pole_points, &p.pole_normals, &p.flat_corner_points] {
        o.push(l.len() as f64);
        for v in l {
            o.extend([v.x, v.y, v.z]);
        }
    }
    o.push(flag(p.crv_class_u.is_some()));
    o.push(flag(p.crv_class_v.is_some()));
    o.push(f64::from(p.bad_crossed_seam_dir as i32));
    o.push(f64::from(p.bad_missing_seam_dir as i32));
    o.push(f64::from(p.bad_near_miss_seam_dir as i32));
    o.push(p.closed_loop_cnt as f64);
    o.push(p.loop_props.len() as f64);
}

#[allow(clippy::too_many_lines)]
fn rust_heal(
    brep: &BIn,
    validate_only: bool,
    init_gaps: bool,
    targets: &Targets,
    voxels: [u64; 3],
    steps: &[Step],
) -> Result<Output, HarnessError> {
    let (path, block) = block_of(brep, validate_only, init_gaps)?;
    let (ex, n) = export_of(&path, &block)?;
    let Some(n) = n else {
        // the B-rep could not be built: the shim reports the same `-1 status`
        return Ok(Output::new().values("out", ex));
    };
    // Run only on scripts the C++ guard accepts: the voxel counts size the voxel model.
    ffi::check_heal(&args_of(targets, voxels, steps, n))
        .map_err(|e| harness(format!("heal script: {e}")))?;
    let c = counts(&ex).ok_or_else(|| harness("export counts"))?;
    let (mut b, exact) = import(&ex)?;
    if exact.iter().any(|x| !x) {
        return Err(harness("a face surface was not imported exactly"));
    }
    // each vertex's vertex use order and the cached gaps, as upstream has them
    let extra = ffi::vertex_lists(&path, &block).map_err(|e| harness(format!("c++ lists: {e}")))?;
    restore(&mut b, &extra, c)?;

    let mut k = Kernel::default();
    let mut marks = Marks::default();
    let (vs, es, fs) = targets_of(targets, n);
    let mut h: HealData<H> = HealData::with_targets(&vs, &es, &fs);
    h.x_cnt = voxels[0] as usize;
    h.y_cnt = voxels[1] as usize;
    h.z_cnt = voxels[2] as usize;

    let mut o = vec![0.0];
    let mut stop = false;
    for s in steps {
        let code = match s {
            Step::CacheEdgeProps => 0,
            Step::CacheVertexProps => 1,
            Step::CacheFacePropsGaps => 2,
            Step::FixTolSizes => 3,
            Step::CacheCoinVertices => 4,
            Step::CacheDegenFaces => 5,
            Step::CacheCoinEdges => 6,
            Step::FixCoinEdges => 7,
            Step::CacheMissedEdgeXsects => 8,
            Step::FixMissedEdgeXsects => 9,
            Step::FixUncontainedEdges => 10,
            Step::FixBadGaps => 11,
            Step::RemoveVertexProps { .. } => 12,
            Step::RemoveEdgeProps { .. } => 13,
            Step::RemoveFaceProps { .. } => 14,
            Step::PropsForTgts { .. } => 15,
            Step::FreeTargets => 16,
            Step::LoadTargets { .. } => 17,
            Step::SetOldCounts { .. } => 18,
            Step::SetDoneHealOp { .. } => 19,
            Step::ResetEdgeProps { .. } => 20,
            Step::ResetVertexProps { .. } => 21,
            Step::SetupEmptyProbArrays => 22,
            Step::Reset { .. } => 23,
            Step::MoveToNotYet { .. } => 24,
            Step::CacheFacePropsStage2 => 25,
            Step::CacheFacePropsStage3 => 26,
            Step::FixBadSheets => 27,
            Step::MakeUvTrimCurves { .. } => 28,
            Step::FixInfiniteRegion => 29,
            Step::HealBrep { .. } => 30,
            Step::FaceSnapshot => 31,
            Step::HealTargets { .. } => 32,
            Step::UpdateTargets { .. } => 33,
            Step::FixCoinVertices => 34,
            Step::SeedDegenFaces => 41,
            Step::SeedFaceProblems { .. } => 42,
            Step::FixDegenFaces => 35,
            Step::FixDegenEdges => 36,
            Step::FixMoveSeam => 37,
            Step::FixSplitEdgesAtSeam => 38,
            Step::FixSplitFaceAtSeams => 39,
            Step::UpdateTracked { .. } => 40,
        };
        let last_without_state = matches!(s, Step::HealBrep { mode, .. } if *mode != 0)
            || matches!(s, Step::HealTargets { .. })
            || match s {
                Step::UpdateTargets { op, add, .. } => {
                    !add.is_empty()
                        && i64::from(*op).min(h.done_heal_op.code())
                            >= HealerOpType::FixCoinVertices.code()
                }
                Step::FixCoinVertices => h.bad_vertex_props_coin_vertices.base.len() >= 2,
                Step::FixDegenFaces => !h.bad_face_props_degen_faces.is_empty(),
                Step::FixDegenEdges => !h.bad_edge_props_degen_edges.is_empty(),
                Step::FixMoveSeam => {
                    !h.bad_face_props_missing_seam.is_empty()
                        || !h.bad_face_props_crossed_seam.is_empty()
                        || !h.bad_face_props_near_miss_seam.is_empty()
                }
                Step::FixSplitEdgesAtSeam => !h.bad_face_props_crossed_seam.is_empty(),
                Step::FixSplitFaceAtSeams => !h.bad_face_props_missing_seam.is_empty(),
                _ => false,
            };
        o.push(f64::from(code));
        let r: Result<bool, SolidError> = match s {
            Step::CacheEdgeProps => h.cache_edge_props(&k, &mut b, HealerOpType::CacheEdgeprops),
            Step::CacheVertexProps => {
                h.cache_vertex_props(&k, &mut b, HealerOpType::CacheVertexprops)
            }
            Step::CacheFacePropsGaps => {
                h.cache_face_props_gaps(&k, &mut b, HealerOpType::CacheFacepropsGaps)
            }
            Step::FixTolSizes => h
                .fix_tol_sizes(&mut k, &mut b, HealerOpType::FixTolsizes)
                .map(|()| false),
            Step::CacheCoinVertices => {
                h.cache_coin_vertices(&k, &b, HealerOpType::CacheCoinVertices)
            }
            Step::CacheDegenFaces => h.cache_degen_faces(&k, &mut b, HealerOpType::CacheDegenFaces),
            Step::CacheCoinEdges => Ok(h.cache_coin_edges(HealerOpType::CacheCoinEdges)),
            Step::FixCoinEdges => h.fix_coin_edges(HealerOpType::FixCoinEdges).map(|()| false),
            Step::CacheMissedEdgeXsects => {
                Ok(h.cache_missed_edge_xsects(HealerOpType::CacheMissedEdgexsects))
            }
            Step::FixMissedEdgeXsects => h
                .fix_missed_edge_xsects(HealerOpType::FixMissedEdgexsects)
                .map(|()| false),
            Step::FixUncontainedEdges => h
                .fix_uncontained_edges(HealerOpType::FixUncontainedEdges)
                .map(|()| false),
            Step::FixBadGaps => {
                h.fix_bad_gaps(HealerOpType::FixBadgaps);
                Ok(false)
            }
            Step::RemoveVertexProps { sel } => {
                let alive: Vec<_> = h.vertex_props.alive().map(|(i, _)| i).collect();
                let Some(p) = pick(&alive, *sel) else {
                    o.push(NONE_ALIVE);
                    continue;
                };
                match h.remove_vertex_props_from_lists(p) {
                    Ok((a, c2)) => {
                        o.extend([p.index() as f64, flag(a), flag(c2)]);
                        continue;
                    }
                    Err(e) => Err(e),
                }
            }
            Step::RemoveEdgeProps { sel } => {
                let alive: Vec<_> = h.edge_props.alive().map(|(i, _)| i).collect();
                let Some(p) = pick(&alive, *sel) else {
                    o.push(NONE_ALIVE);
                    continue;
                };
                match h.remove_edge_props_from_lists(p) {
                    Ok((a, c2)) => {
                        o.extend([p.index() as f64, flag(a), flag(c2)]);
                        continue;
                    }
                    Err(e) => Err(e),
                }
            }
            Step::RemoveFaceProps { sel } => {
                let alive: Vec<_> = h.face_props.alive().map(|(i, _)| i).collect();
                let Some(p) = pick(&alive, *sel) else {
                    o.push(NONE_ALIVE);
                    continue;
                };
                match h.remove_face_props_from_lists(p) {
                    Ok((a, c2)) => {
                        o.extend([p.index() as f64, flag(a), flag(c2)]);
                        continue;
                    }
                    Err(e) => Err(e),
                }
            }
            Step::PropsForTgts { which, stale, tgts } => {
                let list: Vec<EntityRef> = tgts
                    .iter()
                    .filter_map(|[kk, i]| entity(*kk, *i, c))
                    .collect();
                o.push(0.0);
                let out: Vec<f64> = match which {
                    0 => h
                        .vertex_props_for_tgts(&list, *stale)
                        .iter()
                        .map(|p| p.index() as f64)
                        .collect(),
                    1 => h
                        .edge_props_for_tgts(&list, *stale)
                        .iter()
                        .map(|p| p.index() as f64)
                        .collect(),
                    _ => h
                        .face_props_for_tgts(&list, *stale)
                        .iter()
                        .map(|p| p.index() as f64)
                        .collect(),
                };
                o.push(out.len() as f64);
                o.extend(out);
                continue;
            }
            Step::FreeTargets => {
                h.free_targets();
                Ok(false)
            }
            Step::LoadTargets { targets } => {
                let (v2, e2, f2) = targets_of(targets, n);
                h.load_targets(&v2, &e2, &f2);
                Ok(false)
            }
            Step::SetOldCounts { counts } => {
                h.old_vertex_cnt = counts[0] as usize;
                h.old_edge_cnt = counts[1] as usize;
                h.old_face_cnt = counts[2] as usize;
                Ok(false)
            }
            Step::SetDoneHealOp { op } => {
                h.done_heal_op =
                    HealerOpType::from_code(i64::from(*op)).unwrap_or(HealerOpType::None);
                Ok(false)
            }
            Step::ResetEdgeProps { sel, metadata } => {
                let alive: Vec<_> = h.edge_props.alive().map(|(i, _)| i).collect();
                if let Some(p) = pick(&alive, *sel) {
                    if let Some(e) = h.edge_props.get_mut(p) {
                        e.reset(*metadata);
                    }
                }
                Ok(false)
            }
            Step::ResetVertexProps { sel } => {
                let alive: Vec<_> = h.vertex_props.alive().map(|(i, _)| i).collect();
                if let Some(p) = pick(&alive, *sel) {
                    if let Some(v) = h.vertex_props.get_mut(p) {
                        v.reset();
                    }
                }
                Ok(false)
            }
            Step::SetupEmptyProbArrays => {
                h.setup_empty_prob_arrays();
                Ok(false)
            }
            Step::Reset { new_obj_arrays } => h.reset(*new_obj_arrays).map(|()| false),
            Step::MoveToNotYet { sel } => {
                let alive: Vec<_> = h.face_props.alive().map(|(i, _)| i).collect();
                let Some(p) = pick(&alive, *sel) else {
                    o.push(NONE_ALIVE);
                    continue;
                };
                match h.move_to_not_yet_problems(p) {
                    Ok(()) => {
                        o.push(p.index() as f64);
                        continue;
                    }
                    Err(e) => Err(e),
                }
            }
            Step::CacheFacePropsStage2 => {
                h.cache_face_props_stage2(&k, &b, HealerOpType::CacheFaceprops2)
            }
            Step::CacheFacePropsStage3 => {
                h.cache_face_props_stage3(&k, &b, HealerOpType::CacheFaceprops3)
            }
            Step::FixBadSheets => {
                let mut lock = MarkLock::new(std::slice::from_mut(&mut k.context), 0, ALL_MARKS)
                    .map_err(|e| harness(format!("mark lock: {e}")))?;
                let r = h.fix_bad_sheets(
                    &mut k,
                    &mut b,
                    &mut marks,
                    &lock,
                    HealerOpType::FixBadsheets,
                );
                lock.release(std::slice::from_mut(&mut k.context))
                    .map_err(|e| harness(format!("mark lock: {e}")))?;
                r.map(|()| false)
            }
            Step::MakeUvTrimCurves { make } => h
                .make_uv_trim_curves(&k, &mut b, *make, HealerOpType::MakeUvtrimcurves)
                .map(|()| false),
            Step::FixInfiniteRegion => h
                .fix_infinite_region(&mut b, HealerOpType::FixInfiniteRegions)
                .map(|()| false),
            Step::HealBrep {
                mode,
                op,
                make,
                targets,
            } => {
                let op = HealerOpType::from_code(i64::from(*op)).unwrap_or(HealerOpType::None);
                let lists = match (mode, targets) {
                    (2, Some(t)) => Some(targets_of(t, n)),
                    (2, None) => Some((Vec::new(), Vec::new(), Vec::new())),
                    _ => None,
                };
                let t = lists
                    .as_ref()
                    .map_or_else(HealBrepTargets::default, |(v, e, f)| HealBrepTargets {
                        vertices: Some(v),
                        edges: Some(e),
                        faces: Some(f),
                    });
                let brep = if *mode == 0 { None } else { Some(&mut b) };
                h.heal_brep(&mut k, brep, op, t, *make).map(|()| false)
            }
            Step::HealTargets { op, make } => {
                let r = h.heal_targets(&mut k, &mut b, op_of(i64::from(*op)), *make);
                o.extend([
                    h.stop_heal_op.code() as f64,
                    h.done_heal_op.code() as f64,
                    h.recursion_count as f64,
                    h.tgt_vertices.len() as f64,
                    h.tgt_edges.len() as f64,
                    h.tgt_faces.len() as f64,
                ]);
                r.map(|()| false)
            }
            Step::UpdateTargets { op, remove, add } => {
                let rm: Vec<_> = remove
                    .iter()
                    .filter_map(|&[kind, sel]| entity(kind, sel, c))
                    .collect();
                let add: Vec<_> = add
                    .iter()
                    .map(|&[kind, sel]| entity(kind, sel, c))
                    .collect();
                if rm.iter().any(|e| match e {
                    EntityRef::Face(f) => {
                        removal_lookup_is_ub(&h.tgt_faces, h.tgt_face_props.len(), f)
                    }
                    EntityRef::Edge(e) => {
                        removal_lookup_is_ub(&h.tgt_edges, h.tgt_edge_props.len(), e)
                    }
                    EntityRef::Vertex(v) => {
                        removal_lookup_is_ub(&h.tgt_vertices, h.tgt_vertex_props.len(), v)
                    }
                    _ => false,
                }) {
                    o.push(NULL_DEREF);
                    stop = true;
                    break;
                }
                let rf = h.face_props_for_tgts(&rm, true);
                let re = h.edge_props_for_tgts(&rm, true);
                let rv = h.vertex_props_for_tgts(&rm, true);
                let faces_after = target_count_after_removals(
                    &h.tgt_faces,
                    rf.iter()
                        .filter_map(|id| h.face_props.get(*id).and_then(|p| p.props.face)),
                );
                let edges_after = target_count_after_removals(
                    &h.tgt_edges,
                    re.iter()
                        .filter_map(|id| h.edge_props.get(*id).and_then(|p| p.edge)),
                );
                let vertices_after = target_count_after_removals(
                    &h.tgt_vertices,
                    rv.iter()
                        .filter_map(|id| h.vertex_props.get(*id).and_then(|p| p.vertex)),
                );
                let drift = faces_after != h.tgt_face_props.len().saturating_sub(rf.len())
                    || edges_after != h.tgt_edge_props.len().saturating_sub(re.len())
                    || vertices_after != h.tgt_vertex_props.len().saturating_sub(rv.len());
                let new_vertices = add
                    .iter()
                    .flatten()
                    .filter(|e| matches!(e, EntityRef::Vertex(v) if b.vertex(*v).is_ok()))
                    .count();
                let unsized_incremental = i64::from(*op).min(h.done_heal_op.code())
                    >= HealerOpType::CacheCoinVertices.code()
                    && vertices_after > 0
                    && new_vertices > 0
                    && h.vertices_in_voxels.bbox().is_init();
                if !add.is_empty() && (drift || unsized_incremental) {
                    Err(SolidError::invalid_input(
                        "difftest::heal_heal_data_p2",
                        "target/props drift or incremental catch-up before voxel initialization",
                    ))
                } else {
                    h.update_target_lists(&mut k, &mut b, &rm, &add, op_of(i64::from(*op)))
                        .map(|()| false)
                }
            }
            Step::SeedFaceProblems { null_face } => {
                for (i, &id) in h.tgt_face_props.iter().enumerate() {
                    let p = &mut h
                        .face_props
                        .props_mut(id)
                        .map_err(|e| harness(e.to_string()))?
                        .props;
                    p.face = if *null_face {
                        None
                    } else {
                        h.tgt_faces.get(i).copied()
                    };
                    p.face_index = i as u64;
                    p.bad_crossed_seam_dir =
                        solidcore::nurbs::b_spline_surface::p1::SurfParamType::Neither;
                    h.bad_face_props_degen_faces
                        .add(&mut h.face_props, id, true)
                        .map_err(|e| harness(e.to_string()))?;
                    h.bad_face_props_crossed_seam
                        .add(&mut h.face_props, id, true)
                        .map_err(|e| harness(e.to_string()))?;
                    h.bad_face_props_missing_seam
                        .add(&mut h.face_props, id, true)
                        .map_err(|e| harness(e.to_string()))?;
                }
                Ok(false)
            }
            Step::SeedDegenFaces => {
                for (i, &id) in h.tgt_face_props.iter().enumerate() {
                    let p = &mut h
                        .face_props
                        .props_mut(id)
                        .map_err(|e| harness(e.to_string()))?
                        .props;
                    p.face = h.tgt_faces.get(i).copied();
                    p.face_index = i as u64;
                    h.bad_face_props_degen_faces
                        .add(&mut h.face_props, id, true)
                        .map_err(|e| harness(e.to_string()))?;
                }
                Ok(false)
            }
            Step::FixCoinVertices => h
                .fix_coin_vertices(&mut k, &mut b, HealerOpType::FixCoinVertices)
                .map(|()| false),
            Step::FixDegenFaces => h
                .fix_degen_faces(&mut k, &mut b, HealerOpType::FixDegenFaces)
                .map(|()| false),
            Step::FixDegenEdges => h
                .fix_degen_edges(&mut k, &mut b, HealerOpType::FixDegenEdges)
                .map(|()| false),
            Step::FixMoveSeam | Step::FixSplitEdgesAtSeam => {
                let mut lock = MarkLock::new(std::slice::from_mut(&mut k.context), 0, ALL_MARKS)
                    .map_err(|e| harness(e.to_string()))?;
                let r = if matches!(s, Step::FixMoveSeam) {
                    h.fix_move_seam(&mut k, &mut b, &mut marks, &lock, HealerOpType::FixMoveseam)
                } else {
                    h.fix_split_edges_at_seam(
                        &mut k,
                        &mut b,
                        &mut marks,
                        &lock,
                        HealerOpType::FixSplitedgeAtseam,
                    )
                };
                lock.release(std::slice::from_mut(&mut k.context))
                    .map_err(|e| harness(e.to_string()))?;
                r.map(|()| false)
            }
            Step::FixSplitFaceAtSeams => h
                .fix_split_face_at_seams(&mut k, &mut b, HealerOpType::FixSplitfaceAtseams)
                .map(|()| false),
            Step::UpdateTracked { active, op } => {
                use solidcore::topology::{topology::TrackTopologyChanges, BrepId};
                let mut tracker = TrackTopologyChanges::new(
                    Some(BrepId::from_index(0)),
                    123,
                    *active,
                    Some(&mut k.context),
                );
                let r = h.update_tracked_targets(
                    &mut k,
                    &mut b,
                    &tracker,
                    |_| None,
                    op_of(i64::from(*op)),
                );
                tracker.stop_tracking(Some(&mut k.context));
                r.map(|()| false)
            }
            Step::FaceSnapshot => {
                o.push(h.face_props.allocated() as f64);
                for kk in 0..h.face_props.allocated() {
                    match h.face_props.get(FacePropsId(kk)) {
                        None => o.push(0.0),
                        Some(e) => {
                            o.push(1.0);
                            put_face_snapshot(&mut o, &e.props);
                        }
                    }
                }
                continue;
            }
        };
        if matches!(s, Step::FixSplitEdgesAtSeam) && r.is_ok() {
            o.push(h.tgt_face_props.len() as f64);
            for &id in &h.tgt_face_props {
                let p = &h
                    .face_props
                    .props(id)
                    .map_err(|e| harness(e.to_string()))?
                    .props;
                o.extend([
                    f64::from(p.bad_crossed_seam_dir as i32),
                    p.bad_crossed_seam.code() as f64,
                    p.been_through_split_edges_at_seam.code() as f64,
                ]);
            }
        }
        match r {
            Ok(new_probs) if last_without_state => {
                o.extend([0.0, flag(new_probs)]);
                stop = true;
                break;
            }
            Ok(new_probs) => o.extend([0.0, flag(new_probs)]),
            Err(e) => match classify(e) {
                Fail::NullDeref => {
                    o.push(NULL_DEREF);
                    stop = true;
                    break;
                }
                Fail::Status(code) => {
                    o.extend([f64::from(code), 0.0]);
                    stop = true;
                    break;
                }
                Fail::Deferred(d) => return Ok(d),
                Fail::Other(m) => return Ok(Output::err(m)),
            },
        }
    }
    if !stop {
        if let Err(e) = dump(&h, &b, &mut o) {
            return Ok(match classify(e) {
                Fail::Deferred(d) => d,
                _ => Output::err("dump failed"),
            });
        }
    }
    Ok(Output::new().values("out", o))
}

pub(crate) fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    match i {
        Inputs::Prob { kind, n, ops } => rust_prob(*kind, *n, ops),
        Inputs::Heal {
            brep,
            validate_only,
            init_gaps,
            targets,
            voxels,
            steps,
        } => rust_heal(brep, *validate_only, *init_gaps, targets, *voxels, steps),
    }
}

// ---- generation -------------------------------------------------------------------------

fn gen_sel(rng: &mut Rng) -> Sel {
    match rng.below(10) {
        0..=5 => Sel::All,
        6 => Sel::Reverse,
        7 => Sel::None,
        _ => {
            let n = rng.below(12) as usize;
            Sel::List((0..n).map(|_| rng.below(1 << 16)).collect())
        }
    }
}

pub(crate) fn gen_targets(rng: &mut Rng) -> Targets {
    Targets {
        // vertex props of face vertices need the vertex/face gap (often deferred):
        // leave the vertices out of a third of the cases
        v: if rng.chance(0.35) {
            Sel::None
        } else {
            gen_sel(rng)
        },
        e: gen_sel(rng),
        f: gen_sel(rng),
    }
}

pub(crate) fn log_uniform(rng: &mut Rng, lo: f64, hi: f64) -> f64 {
    10f64.powf(rng.uniform(lo.log10(), hi.log10()))
}

pub(crate) fn gen_brep_in(rng: &mut Rng) -> BIn {
    match rng.below(20) {
        0..=5 => BIn::Prim(Prim::Box {
            size: [
                log_uniform(rng, 1e-3, 1e3),
                log_uniform(rng, 1e-3, 1e3),
                log_uniform(rng, 1e-3, 1e3),
            ],
        }),
        6 | 7 => BIn::Prim(Prim::Sphere {
            radius: log_uniform(rng, 1e-2, 1e2),
            swap: rng.chance(0.5),
        }),
        _ => BIn::Std(gen_brep(rng)),
    }
}

fn gen_extra(rng: &mut Rng) -> Step {
    match rng.below(14) {
        0 => Step::RemoveVertexProps { sel: rng.below(64) },
        1 => Step::RemoveEdgeProps { sel: rng.below(64) },
        2 => Step::RemoveFaceProps { sel: rng.below(64) },
        3 | 4 => Step::PropsForTgts {
            which: rng.below(3) as u8,
            stale: rng.chance(0.5),
            tgts: (0..rng.below(8))
                .map(|_| [rng.below(10), rng.below(64)])
                .collect(),
        },
        5 => Step::SetOldCounts {
            counts: [rng.below(6), rng.below(10), rng.below(4)],
        },
        6 => Step::SetDoneHealOp {
            op: rng.below(27) as u8,
        },
        7 => Step::ResetEdgeProps {
            sel: rng.below(64),
            metadata: rng.chance(0.5),
        },
        8 => Step::ResetVertexProps { sel: rng.below(64) },
        9 => Step::SetupEmptyProbArrays,
        10 => Step::LoadTargets {
            targets: gen_targets(rng),
        },
        11 => Step::FixUncontainedEdges,
        12 => Step::CacheEdgeProps,
        _ => Step::CacheCoinVertices,
    }
}

fn gen_heal(rng: &mut Rng) -> Inputs {
    let brep = gen_brep_in(rng);
    let mut steps = vec![
        Step::CacheEdgeProps,
        Step::CacheVertexProps,
        Step::CacheFacePropsGaps,
    ];
    if rng.chance(0.1) {
        steps.push(Step::FixTolSizes);
    }
    steps.push(Step::CacheCoinVertices);
    if rng.chance(0.1) {
        steps.push(Step::CacheDegenFaces);
    }
    steps.extend([
        Step::CacheCoinEdges,
        Step::FixCoinEdges,
        Step::CacheMissedEdgeXsects,
        Step::FixMissedEdgeXsects,
        Step::FixUncontainedEdges,
        Step::FixBadGaps,
    ]);
    // insert extra steps (also before the caches)
    for _ in 0..rng.below(6) {
        let at = rng.below(steps.len() as u64 + 1) as usize;
        steps.insert(at, gen_extra(rng));
    }
    // incremental re-run on a tail of the targets
    if rng.chance(0.25) {
        steps.push(Step::SetOldCounts {
            counts: [rng.below(8), rng.below(12), rng.below(6)],
        });
        steps.extend([
            Step::CacheEdgeProps,
            Step::CacheVertexProps,
            Step::CacheCoinVertices,
        ]);
    }
    if rng.chance(0.15) {
        steps.push(Step::FreeTargets);
    }
    let vox = if rng.chance(0.6) {
        [12, 12, 12]
    } else {
        [1 + rng.below(16), 1 + rng.below(16), 1 + rng.below(16)]
    };
    Inputs::Heal {
        brep: Box::new(brep),
        validate_only: rng.chance(0.1),
        init_gaps: rng.chance(0.5),
        targets: gen_targets(rng),
        voxels: vox,
        steps,
    }
}

fn gen_prob(rng: &mut Rng) -> Inputs {
    let kind = rng.below(4) as u8;
    let n = 1 + rng.below(8);
    let max_link: u64 = match kind {
        2 => 2,
        3 => 3,
        _ => 18,
    };
    let mut ops: Vec<[i64; 4]> = Vec::new();
    if rng.chance(0.85) {
        ops.push([0, rng.below(27) as i64, rng.below(max_link) as i64, 0]);
    }
    let obj = |rng: &mut Rng| rng.below(n) as i64;
    for _ in 0..rng.below(30) {
        let op = match rng.below(20) {
            0..=6 => {
                let o = obj(rng);
                let arg = match kind {
                    2 => rng.below(6) as i64,
                    3 => rng.below(4) as i64,
                    _ => i64::from(rng.chance(0.85)),
                };
                [1, o, arg, 0]
            }
            7..=10 => [2, obj(rng), 0, 0],
            11 => [3, 0, 0, 0],
            12 => [4, 0, 0, 0],
            13 => [5, 0, 0, 0],
            14 | 15 => [6, obj(rng), rng.below(18) as i64, rng.below(3) as i64],
            16 => [7, rng.below(3) as i64, 0, 0],
            17 => [8, rng.below(100) as i64, 0, 0],
            18 => [9, rng.below(50) as i64, 0, 0],
            _ => [
                0,
                rng.below(27) as i64,
                rng.below(max_link + 1) as i64 - 1,
                0,
            ],
        };
        ops.push(op);
    }
    Inputs::Prob { kind, n, ops }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "heal.heal_data_p2"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let c = if rng.chance(0.25) {
            gen_prob(rng)
        } else {
            gen_heal(rng)
        };
        serde_json::to_value(c).ok()
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
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Object counts the C++ guard refuses are a harness error on the Rust side too,
    /// not a capacity-overflow panic or a 32 GiB object list (issue #181).
    #[test]
    fn prob_refuses_unguarded_object_counts() {
        for n in [
            0,
            ffi::MAX_OBJECTS as u64 + 1,
            1 << 31,
            1 << 32,
            i64::MAX as u64,
            u64::MAX,
        ] {
            let i = Inputs::Prob {
                kind: 1,
                n,
                ops: Vec::new(),
            };
            assert!(run_rust(&i).is_err(), "n = {n}");
        }
    }

    /// Generated problem array scripts pass the guard and still run.
    #[test]
    fn prob_runs_generated_scripts() {
        let mut rng = Rng::new(7);
        for _ in 0..200 {
            let i = gen_prob(&mut rng);
            assert!(run_rust(&i).is_ok(), "{i:?}");
        }
    }
}
