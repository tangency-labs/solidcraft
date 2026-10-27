//! Runner for unit `topology.brep_p1`. C++: `solidcore_ffi::units::topology_brep_p1`.
//! Rust: `solidcore::topology::brep::p1`.
//!
//! As the `topology.edgeuse_p1` runner, the C++ side builds a real `SmBrep` per call
//! (an upstream `.smb` file, a wire edge, a face, or the `prog_test` wire network). The
//! Rust side builds every non-file source itself with the Rust kernel
//! ([`crate::own_builder`], renumbered into the shim's traversal order); for an `.smb`
//! file it imports the graph the shim exports in traversal order into a [`Brep`]
//! (the shared import of `topology.edgeuse_p1`, plus this unit's extras: the infinite
//! region, the editing flag, the size estimates and each vertex's vertex use list in
//! its native order). Every op's C++ output after the build status is compared with the
//! Rust side's as one float array (`out`): statuses (`0`, an `SmStatus` code, `-2`
//! where upstream dereferences null), traversal indices, flags and values.
//!
//! Selectors are taken modulo the entity count (`-1` selecting null where allowed); an
//! op needing an entity the B-rep lacks is rejected (nothing runs on either side).
//!
//! **Inexact geometry.** The export carries plain B-spline curves and `SmLine`s and
//! B-spline surfaces, planes and supported analytic surface records exactly.
//! Ellipse/circle edges retain their native kind, stored NURB and trim parameters.
//! Unsupported or lossy records retain their B-spline part. The ops evaluating curves and surfaces (`topological` lists,
//! `check_uv`) defer when the geometry they touch is inexact (as `topology.edgeuse_p1`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use std::collections::{BTreeMap, BTreeSet};

use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{Attribute, AttributeStore, Behavior, ObjectKey};
use solidcore::geom::context::MarkType;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::object::ObjectRef;
use solidcore::geom::tol;
use solidcore::geom::vector3d::Vec3;
use solidcore::heal::face_props::HealerOpType;
use solidcore::kernel::Kernel;
use solidcore::math::Point2;
use solidcore::nurbs::b_spline_surface::SurfParamType;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::p2::SurfaceKind;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep_cache::{arena_identity, CacheObject};
use solidcore::topology::edge::AttributeFilter;
use solidcore::topology::proto_topology::p1::CopyBrepMap;
use solidcore::topology::topology_traverser::{Entity, Marks};
use solidcore::topology::{
    Brep, EdgeId, EdgeuseId, EdgeuseOwner, FaceId, FaceuseId, LoopId, LoopuseId, LoopuseStart,
    RegionId, ShellId, VertexId, VertexuseId, VertexuseOwner,
};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::topology_brep_p1::{self as ffi, Op, N_EXTRA_SCALARS, TARGET_TYPES};

use super::topology_edgeuse_p1::{
    brep_block_of, bspline_of, flag, gen_brep, gen_curve, import_raw_with_types, path_of,
    push_curve, sel, surface_of, BrepIn, CrvIn, ImportTypes, FILES,
};
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The difftest runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// "Upstream would dereference null" in a status slot.
const NULL_DEREF: f64 = -2.0;
/// The shim's HANG-39 rejection of a corner blend (C++ not run; see `run_cpp`).
const HANG34_GUARD: f64 = -34.0;

/// The type codes `IsKindOf` is asked about (as the shim's `KIND_TYPES`).
const KIND_TYPES: [i64; 9] = [16011, 16050, 12060, 11001, 16012, 16001, 16010, 16999, 0];

/// `SmBSplineSurface_TYPE`, `SmPlane_TYPE`; `SmBSplineCurve_TYPE`, `SmLine_TYPE`.
const EXACT_SURFACES: [i64; 2] = [15002, 15011];
const EXACT_CURVES: [i64; 2] = [14033, 14010];

/// The deferral label of cases whose geometry the shared export of
/// `topology.edgeuse_p1` flattens to B-spline parts (analytic surfaces and curves):
/// not an unported unit but a harness limitation, so the ops evaluating that geometry
/// are not compared on it (the C++ side is not run for them either).
const HARNESS_ANALYTIC_IMPORT: &str = "harness:analytic-geometry-export";

// ---- inputs -----------------------------------------------------------------------------

/// An `sm_SetObjectMap` call: `orig -> copy` (targets as `[type, index]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ObjSet {
    orig: [i64; 2],
    copy: [i64; 2],
    #[serde(default)]
    copy_map: bool,
    #[serde(default)]
    reverse: bool,
}

fn minus_one() -> i64 {
    -1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    /// `which`: 0 regions, 1 faces, 2 edges, 3 vertices, 4 wire edges, 5 topological
    /// edges, 6 topological vertices, 7 surfaces, 8 curves, 9 shells; `attr` (-1
    /// none) is attached to `targets` first.
    Lists {
        which: u8,
        #[serde(default = "minus_one")]
        attr: i64,
        #[serde(default)]
        targets: Vec<[i64; 2]>,
    },
    Query {
        ty: i64,
        index: i64,
    },
    /// `kind`: 0 `SetThisModelSizeEstimate`, 1 `SetThisLargeSmallSizeRatio`.
    SetSize {
        kind: u8,
        value: f64,
    },
    /// `set`: -2 skip `SetInfiniteRegion`, -1 null, else a region; `mode`: 0 none, 1
    /// `ResetInfiniteRegion(reset, flag)`, 2 `SetRegionIsVoidFlagsForNestedSolids`.
    SetInf {
        set: i64,
        mode: u8,
        reset: i64,
        flag: u8,
        /// A region whose raw void flag is set to `UNSURE` (2) first (`-1`: none).
        #[serde(default = "minus_one")]
        unsure: i64,
    },
    Degenerate {
        tol: f64,
    },
    AddDeleted {
        n: u8,
    },
    Vote {
        regions: Vec<i64>,
    },
    ObjMap {
        sets: Vec<ObjSet>,
        gets: Vec<[i64; 2]>,
    },
    AddAttrs {
        target: [i64; 2],
        ids: Vec<i64>,
        indexed: Vec<i64>,
    },
    ReflectEu {
        edge: i64,
    },
    ReflectShell {
        face: i64,
    },
    RmLoopVu {
        vu: i64,
    },
    MarkLoops {
        #[serde(rename = "loop")]
        loop_: i64,
        level: u64,
    },
    /// `mark`: an `SmMarkType` code (1, 2, 4, 8, 16); `premark` marks the face first.
    SwapFaceUv {
        face: i64,
        mark: u8,
        premark: bool,
    },
    CheckUv {
        eu: i64,
    },
    LockBox,
    FindSide {
        curve: CrvIn,
        p: [f64; 3],
        dir: [f64; 3],
        tol: f64,
    },
    FindSector {
        face: i64,
        guess: Option<[f64; 2]>,
        p: [f64; 3],
        dir: [f64; 3],
    },
    UpdateIvls {
        vertex: i64,
    },
    /// `seam`: an `SmSurfParamType` code (0 U, 1 V, 2 UNKNOWN, 3 BOTH, 4 NEITHER, 5 UMIN,
    /// 6 VMIN, 7 UMAX, 8 VMAX).
    Redistribute {
        edge: i64,
        has_uv: bool,
        seam: u8,
        dom: [f64; 4],
    },
    /// `orient`: 0 unknown, 1 same, 2 opposite (`SmOrientType`).
    SplitUv {
        eu_orig: i64,
        eu_new: i64,
        p: [f64; 3],
        t: f64,
        scaled_zero: f64,
        orient: u8,
        for_tess: bool,
    },
    BrepIntersect {
        tol: f64,
        ang: f64,
        want_points: bool,
    },
    /// Two independently constructed finite faces; no C++ graph import.
    BrepIntersect2 {
        other: BrepIn,
        tol: f64,
        ang: f64,
        want_points: bool,
    },
    CornerBlend {
        eus: Vec<i64>,
        tangent: Option<Vec<bool>>,
        tol: f64,
        ang: f64,
    },
    EdgeEdgeBlend {
        flag: u8,
        eu1: i64,
        eu2: i64,
        dir: u8,
    },
    /// A ruled `CreateEdgeEdgeBlend` with `eu2` in a second B-rep `other` (not a file
    /// block): each rail is read from its own arena.
    EdgeEdgeBlend2 {
        other: BrepIn,
        eu1: i64,
        eu2: i64,
        dir: u8,
    },
    ParallelProj {
        curve: CrvIn,
        vec: [f64; 3],
        dist: Option<f64>,
        tol: Option<f64>,
        ang: Option<f64>,
        want_3d: bool,
        want_uv: bool,
        want_faces: bool,
    },
    /// `healer_op`: an `SmHealerOpType` code, 0 (`SM_HO_NONE`) to 26 (`SM_HO_ALL`); the
    /// FFI rejects anything above (upstream would run every step for an out-of-range code,
    /// the Rust enum has no such value). `SmBrep::HealBrep` always returns `SM_SUCCESS`, so
    /// the compared result is the healed B-rep ([`put_healed`]).
    Heal {
        healer_op: u8,
        make_uv: bool,
    },
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

// ---- selectors --------------------------------------------------------------------------

/// The export slot of a target type code.
fn slot_of(ty: i64) -> Option<usize> {
    TARGET_TYPES
        .iter()
        .find(|(code, _)| *code == ty)
        .map(|&(_, s)| s)
}

/// A target resolved modulo its kind's count.
fn target(t: [i64; 2], c: [usize; 10]) -> [i64; 2] {
    let [ty, i] = t;
    let slot = slot_of(ty).or(match ty {
        ffi::SURFACE_TYPE => Some(3),
        ffi::CURVE_TYPE => Some(7),
        _ => None,
    });
    match slot {
        Some(s) => [ty, sel(i, c.get(s).copied().unwrap_or(0))],
        None => [ty, if i < 0 { -1 } else { 0 }],
    }
}

/// The selectors of an op resolved against the B-rep's counts; `None` when the op needs
/// an entity the B-rep lacks.
fn resolve(op: &OpIn, c: [usize; 10]) -> Option<OpIn> {
    let [n_r, _, _, n_f, _, n_l, n_eu, n_e, n_vu, _] = c;
    let need = |n: usize, s: i64| if n == 0 { None } else { Some(sel(s, n).max(0)) };
    Some(match op.clone() {
        OpIn::Lists {
            which,
            attr,
            targets,
        } => OpIn::Lists {
            which: which % 10,
            attr,
            targets: targets.into_iter().map(|t| target(t, c)).collect(),
        },
        OpIn::Query { ty, index } => {
            let [ty, index] = target([ty, index], c);
            OpIn::Query { ty, index }
        }
        OpIn::SetSize { kind, value } => OpIn::SetSize {
            kind: kind % 2,
            value,
        },
        OpIn::SetInf {
            set,
            mode,
            reset,
            flag,
            unsure,
        } => OpIn::SetInf {
            set: if set < 0 { set.max(-2) } else { sel(set, n_r) },
            mode: mode % 3,
            reset: sel(reset, n_r),
            // the raw SmSetRegionVoidType code (lesson 22: the FFI and the Rust decode
            // refuse anything but 0..=2)
            flag,
            unsure: if unsure < 0 { -1 } else { sel(unsure, n_r) },
        },
        OpIn::Vote { regions } => OpIn::Vote {
            regions: regions.into_iter().map(|r| sel(r, n_r)).collect(),
        },
        OpIn::ObjMap { sets, gets } => OpIn::ObjMap {
            sets: sets
                .into_iter()
                .map(|s| ObjSet {
                    orig: target(s.orig, c),
                    copy: target(s.copy, c),
                    ..s
                })
                .collect(),
            gets: gets.into_iter().map(|t| target(t, c)).collect(),
        },
        OpIn::AddAttrs {
            target: t,
            ids,
            indexed,
        } => {
            let n = ids.len();
            OpIn::AddAttrs {
                target: target(t, c),
                indexed: if n == 0 {
                    Vec::new()
                } else {
                    indexed.into_iter().map(|i| sel(i, n).max(0)).collect()
                },
                ids,
            }
        }
        OpIn::ReflectEu { edge } => OpIn::ReflectEu {
            edge: need(n_e, edge)?,
        },
        OpIn::ReflectShell { face } => OpIn::ReflectShell {
            face: need(n_f, face)?,
        },
        OpIn::RmLoopVu { vu } => OpIn::RmLoopVu {
            vu: need(n_vu, vu)?,
        },
        OpIn::MarkLoops { loop_, level } => OpIn::MarkLoops {
            loop_: need(n_l, loop_)?,
            level: level % 1_000_000,
        },
        OpIn::SwapFaceUv {
            face,
            mark,
            premark,
        } => OpIn::SwapFaceUv {
            face: need(n_f, face)?,
            // the raw SmMarkType code: the FFI refuses anything but 1/2/4/8/16 and the
            // Rust decode (mark_type) errors on it, so an unknown code never runs
            mark,
            premark,
        },
        OpIn::CheckUv { eu } => OpIn::CheckUv {
            eu: need(n_eu, eu)?,
        },
        OpIn::FindSector {
            face,
            guess,
            p,
            dir,
        } => OpIn::FindSector {
            face: need(n_f, face)?,
            guess,
            p,
            dir,
        },
        OpIn::UpdateIvls { vertex } => OpIn::UpdateIvls {
            vertex: need(c[9], vertex)?,
        },
        OpIn::Redistribute {
            edge,
            has_uv,
            seam,
            dom,
        } => OpIn::Redistribute {
            edge: need(n_e, edge)?,
            has_uv,
            seam: seam % 9,
            dom,
        },
        OpIn::SplitUv {
            eu_orig,
            eu_new,
            p,
            t,
            scaled_zero,
            orient,
            for_tess,
        } => OpIn::SplitUv {
            eu_orig: need(n_eu, eu_orig)?,
            eu_new: need(n_eu, eu_new)?,
            p,
            t,
            scaled_zero,
            orient,
            for_tess,
        },
        OpIn::CornerBlend {
            eus,
            tangent,
            tol,
            ang,
        } => {
            if n_eu == 0 && !eus.is_empty() {
                return None;
            }
            OpIn::CornerBlend {
                eus: eus.into_iter().map(|e| sel(e, n_eu).max(0)).collect(),
                tangent,
                tol,
                ang,
            }
        }
        OpIn::EdgeEdgeBlend {
            flag,
            eu1,
            eu2,
            dir,
        } => OpIn::EdgeEdgeBlend {
            flag: flag % 4,
            eu1: sel(eu1, n_eu),
            eu2: sel(eu2, n_eu),
            dir: dir % 3,
        },
        // (eu2 indexes the second B-rep: the FFI checks it against that B-rep's counts)
        OpIn::EdgeEdgeBlend2 {
            other,
            eu1,
            eu2,
            dir,
        } => {
            if n_eu == 0 {
                return None;
            }
            OpIn::EdgeEdgeBlend2 {
                other,
                eu1: sel(eu1, n_eu),
                eu2,
                dir: dir % 3,
            }
        }
        other @ (OpIn::Degenerate { .. }
        | OpIn::AddDeleted { .. }
        | OpIn::LockBox
        | OpIn::FindSide { .. }
        | OpIn::BrepIntersect { .. }
        | OpIn::BrepIntersect2 { .. }
        | OpIn::ParallelProj { .. }
        | OpIn::Heal { .. }) => other,
    })
}

/// An op naming an entity kind the B-rep lacks (e.g. a face op on a wire B-rep): no code
/// runs on either side, so the case is rejected (not counted as a pass).
/// Inputs the FFI refuses, rejected on both sides before anything runs: a non-positive
/// `ParallelProj` angle tolerance or an approximation tolerance below the smallest normal
/// double (non-positive: the projector subdivides to it; subnormal: upstream spins,
/// HANG-32). (A `CornerBlend` tangency tolerance below 1e-20 is rejected by the shim,
/// only where upstream reaches the cross-tangent approximation: HANG-39.)
fn refused_input(op: &OpIn) -> Option<Output> {
    match op {
        OpIn::ParallelProj { tol, ang, .. }
            if tol.is_some_and(|t| t < f64::MIN_POSITIVE) || ang.is_some_and(|a| a <= 0.0) =>
        {
            Some(Output::rejected(
                "projection tolerance the FFI refuses (non-positive, or subnormal: HANG-32)",
            ))
        }
        _ => None,
    }
}

fn no_entity() -> Output {
    Output::rejected("no entity of the op's kind in this B-rep: nothing runs on either side")
}

fn ints_of(v: &[i64]) -> Vec<f64> {
    v.iter().map(|&x| x as f64).collect()
}

fn list_of(v: &[i64]) -> Vec<f64> {
    let mut out = vec![v.len() as f64];
    out.extend(ints_of(v));
    out
}

/// The shim's op code and arguments of a resolved op.
fn op_args(op: &OpIn) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut a = Vec::new();
    let code = match op {
        OpIn::BrepIntersect2 { .. } => {
            return Err(harness(
                "cross-body observation bypasses legacy graph import",
            ));
        }
        OpIn::Lists {
            which,
            attr,
            targets,
        } => {
            a.extend([f64::from(*which), *attr as f64, targets.len() as f64]);
            for t in targets {
                a.extend(ints_of(t));
            }
            Op::Lists
        }
        OpIn::Query { ty, index } => {
            a.extend([*ty as f64, *index as f64]);
            Op::Query
        }
        OpIn::SetSize { kind, value } => {
            a.extend([f64::from(*kind), *value]);
            Op::SetSize
        }
        OpIn::SetInf {
            set,
            mode,
            reset,
            flag,
            unsure,
        } => {
            a.extend([
                *set as f64,
                f64::from(*mode),
                *reset as f64,
                f64::from(*flag),
                *unsure as f64,
            ]);
            Op::SetInf
        }
        OpIn::Degenerate { tol } => {
            a.push(*tol);
            Op::Degenerate
        }
        OpIn::AddDeleted { n } => {
            a.push(f64::from(*n));
            Op::AddDeleted
        }
        OpIn::Vote { regions } => {
            a.extend(list_of(regions));
            Op::Vote
        }
        OpIn::ObjMap { sets, gets } => {
            a.push(sets.len() as f64);
            for s in sets {
                a.extend(ints_of(&s.orig));
                a.extend(ints_of(&s.copy));
                a.extend([flag(s.copy_map), flag(s.reverse)]);
            }
            a.push(gets.len() as f64);
            for g in gets {
                a.extend(ints_of(g));
            }
            Op::ObjMap
        }
        OpIn::AddAttrs {
            target,
            ids,
            indexed,
        } => {
            a.extend(ints_of(target));
            a.extend(list_of(ids));
            a.extend(list_of(indexed));
            Op::AddAttrs
        }
        OpIn::ReflectEu { edge } => {
            a.push(*edge as f64);
            Op::ReflectEu
        }
        OpIn::ReflectShell { face } => {
            a.push(*face as f64);
            Op::ReflectShell
        }
        OpIn::RmLoopVu { vu } => {
            a.push(*vu as f64);
            Op::RmLoopVu
        }
        OpIn::MarkLoops { loop_, level } => {
            a.extend([*loop_ as f64, *level as f64]);
            Op::MarkLoops
        }
        OpIn::SwapFaceUv {
            face,
            mark,
            premark,
        } => {
            a.extend([*face as f64, f64::from(*mark), flag(*premark)]);
            Op::SwapFaceUv
        }
        OpIn::CheckUv { eu } => {
            a.push(*eu as f64);
            Op::CheckUv
        }
        OpIn::LockBox => Op::LockBox,
        OpIn::FindSide { curve, p, dir, tol } => {
            push_curve(&mut a, curve)?;
            a.extend(p);
            a.extend(dir);
            a.push(*tol);
            Op::FindSide
        }
        OpIn::FindSector {
            face,
            guess,
            p,
            dir,
        } => {
            a.push(*face as f64);
            a.push(flag(guess.is_some()));
            a.extend(guess.unwrap_or([0.0, 0.0]));
            a.extend(p);
            a.extend(dir);
            Op::FindSector
        }
        OpIn::UpdateIvls { vertex } => {
            a.push(*vertex as f64);
            Op::UpdateIvls
        }
        OpIn::Redistribute {
            edge,
            has_uv,
            seam,
            dom,
        } => {
            a.extend([*edge as f64, flag(*has_uv), f64::from(*seam)]);
            a.extend(dom);
            Op::Redistribute
        }
        OpIn::SplitUv {
            eu_orig,
            eu_new,
            p,
            t,
            scaled_zero,
            orient,
            for_tess,
        } => {
            a.extend([*eu_orig as f64, *eu_new as f64]);
            a.extend(p);
            a.extend([*t, *scaled_zero, f64::from(*orient), flag(*for_tess)]);
            Op::SplitUv
        }
        OpIn::BrepIntersect {
            tol,
            ang,
            want_points,
        } => {
            a.extend([*tol, *ang, flag(*want_points)]);
            Op::BrepIntersect
        }
        OpIn::CornerBlend {
            eus,
            tangent,
            tol,
            ang,
        } => {
            a.extend(list_of(eus));
            a.push(flag(tangent.is_some()));
            let tans = tangent.clone().unwrap_or_default();
            a.push(tans.len() as f64);
            a.extend(tans.iter().map(|&b| flag(b)));
            a.extend([*tol, *ang]);
            Op::CornerBlend
        }
        OpIn::EdgeEdgeBlend {
            flag: f,
            eu1,
            eu2,
            dir,
        } => {
            a.extend([f64::from(*f), *eu1 as f64, *eu2 as f64, f64::from(*dir)]);
            Op::EdgeEdgeBlend
        }
        OpIn::EdgeEdgeBlend2 {
            other,
            eu1,
            eu2,
            dir,
        } => {
            a.extend(brep_block_of(Some(other), false, false)?);
            a.extend([*eu1 as f64, *eu2 as f64, f64::from(*dir)]);
            Op::EdgeEdgeBlend2
        }
        OpIn::ParallelProj {
            curve,
            vec,
            dist,
            tol,
            ang,
            want_3d,
            want_uv,
            want_faces,
        } => {
            push_curve(&mut a, curve)?;
            a.extend(vec);
            for v in [dist, tol, ang] {
                a.push(flag(v.is_some()));
                a.push(v.unwrap_or(1.0));
            }
            a.extend([flag(*want_3d), flag(*want_uv), flag(*want_faces)]);
            Op::ParallelProj
        }
        OpIn::Heal { healer_op, make_uv } => {
            a.extend([f64::from(*healer_op), flag(*make_uv)]);
            Op::Heal
        }
    };
    Ok((code, a))
}

// ---- export / import --------------------------------------------------------------------

fn brep_block(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    brep_block_of(i.brep.as_ref(), i.validate_only, i.init_gaps)
}

/// The C++ export of the case's B-rep (extras, then the graph).
fn export(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let path = path_of(i.brep.as_ref())?;
    let block = brep_block(i)?;
    crate::export_memo::export("topology_brep_p1", &[&path], &block, || {
        ffi::eval(Op::Export, &path, &block)
    })
    .map_err(|e| harness(format!("c++ export: {e}")))
}

fn build_failure(export: &[f64]) -> Output {
    Output::err(format!(
        "brep_build_status_{}",
        export.get(1).copied().unwrap_or(0.0)
    ))
}

/// The Rust B-rep of an export with this unit's extras applied, and the geometry
/// types of the import, with its cached gaps refilled on the finished B-rep
/// ([`super::topology_vertexuse::refill_gaps`]).
pub(crate) fn import(ex: &[f64], graph_at: usize) -> Result<(Brep, ImportTypes), HarnessError> {
    let (mut b, types) = import_raw(ex, graph_at)?;
    super::topology_vertexuse::refill_gaps(&mut b);
    Ok((b, types))
}

/// [`import`] without refilling the cached gaps (they stay marked as set with
/// `SM_UNDEF_DOUBLE` placeholder values), for the C++ side, which only resolves
/// selectors on the import.
pub(crate) fn import_raw(ex: &[f64], graph_at: usize) -> Result<(Brep, ImportTypes), HarnessError> {
    let mut graph = vec![0.0];
    graph.extend_from_slice(ex.get(graph_at..).ok_or_else(|| harness("short export"))?);
    let (mut b, types) = import_raw_with_types(&graph)?;
    let x = ex
        .get(1..1 + N_EXTRA_SCALARS)
        .ok_or_else(|| harness("short extras"))?;
    let [inf, editing, est, ratio, head_is_inf] = *x else {
        return Err(harness("short extras"));
    };
    if inf >= 0.0 {
        b.set_infinite_region(Some(RegionId::from_index(inf as u32)));
    }
    // a freshly built SmBrep: its head region is the infinite one and its size
    // estimates are the defaults, which the Rust B-rep starts with
    if editing != 0.0 || est != tol::USE_DEFAULT || ratio != tol::USE_DEFAULT || head_is_inf != 1.0
    {
        return Err(harness("export: unexpected SmBrep state"));
    }
    // the exported head of the region list (`m_pList`) is the infinite region (or NULL
    // on a wire B-rep without regions), checked just above
    b.set_region_list_head(b.infinite_region());
    let n_v = *ex
        .get(1 + N_EXTRA_SCALARS)
        .ok_or_else(|| harness("short extras"))? as usize;
    let mut at = 2 + N_EXTRA_SCALARS;
    for v in 0..n_v {
        let n = *ex.get(at).ok_or_else(|| harness("short vertex list"))? as usize;
        let list: Vec<VertexuseId> = ex
            .get(at + 1..at + 1 + n)
            .ok_or_else(|| harness("short vertex list"))?
            .iter()
            .map(|&i| VertexuseId::from_index(i as u32))
            .collect();
        at += 1 + n;
        b.vertex_mut(VertexId::from_index(v as u32))
            .map_err(|e| harness(e.to_string()))?
            .vertexuses = list;
    }
    // the B-rep's face, edge and vertex lists in their list order
    let read_list = |at: &mut usize| -> Result<Vec<u32>, HarnessError> {
        let n = *ex.get(*at).ok_or_else(|| harness("short list"))? as usize;
        let v = ex
            .get(*at + 1..*at + 1 + n)
            .ok_or_else(|| harness("short list"))?
            .iter()
            .map(|&i| i as u32)
            .collect();
        *at += 1 + n;
        Ok(v)
    };
    let fl = read_list(&mut at)?;
    let el = read_list(&mut at)?;
    let vl = read_list(&mut at)?;
    b.set_face_list(fl.into_iter().map(FaceId::from_index).collect())
        .map_err(|e| harness(format!("face list: {e}")))?;
    b.set_edge_list(el.into_iter().map(EdgeId::from_index).collect())
        .map_err(|e| harness(format!("edge list: {e}")))?;
    b.set_vertex_list(vl.into_iter().map(VertexId::from_index).collect())
        .map_err(|e| harness(format!("vertex list: {e}")))?;
    // the geometry's owners (`SmSurface::GetOwner` / `SmCurve::GetOwner`, set by the
    // B-rep build upstream): the kernel's arena identities
    let edge_ids: Vec<EdgeId> = b.edges().ids().collect();
    for id in edge_ids {
        if let Some(curve) = b
            .edge_mut(id)
            .map_err(|e| harness(e.to_string()))?
            .curve
            .as_mut()
        {
            let _ = curve.set_owner(
                Some(arena_identity(CacheObject::Edge(id))),
                Some(arena_identity(CacheObject::Brep)),
            );
        }
    }
    let face_ids: Vec<FaceId> = b.faces().ids().collect();
    for id in face_ids {
        if let Some(surface) = b
            .face_mut(id)
            .map_err(|e| harness(e.to_string()))?
            .surface
            .as_mut()
        {
            let _ = surface.set_owner(Some(arena_identity(CacheObject::Face(id))));
        }
        // the face's tolerance, as the face upstream answers GetTolerance with
        b.sync_face_surface_tolerance(id)
            .map_err(|e| harness(e.to_string()))?;
    }
    // the shared import leaves `m_pFU` unset: the face's upward (SAME) face use
    let ups: Vec<(FaceId, FaceuseId)> = b
        .faceuses()
        .iter()
        .filter(|(_, r)| r.orientation == OrientType::Same)
        .filter_map(|(id, r)| r.face.map(|f| (f, id)))
        .collect();
    for (f, fu) in ups {
        if let Ok(face) = b.face_mut(f) {
            if face.faceuse.is_none() {
                face.faceuse = Some(fu);
            }
        }
    }
    // the import names every geometry owner by arena identity (above); record that
    // naming (and each owner) on the B-rep, so library queries that name geometry under
    // another scheme's identities (a glue's notification identities, overridden by
    // recorded owners) resolve the owners instead of failing internally
    b.use_arena_naming().map_err(|e| harness(e.to_string()))?;
    Ok((b, types))
}

// ---- C++ side ---------------------------------------------------------------------------

fn run_cpp(i: &Inputs) -> Result<Output, HarnessError> {
    if let OpIn::BrepIntersect2 {
        other,
        tol,
        ang,
        want_points,
    } = &i.op
    {
        let mut input = cross_body_input(i, other)?;
        input.extend([*tol, *ang, flag(*want_points)]);
        let out = ffi::eval(Op::BrepIntersect2, "", &input)
            .map_err(|e| harness(format!("c++ cross-body: {e}")))?;
        return intersection_output(&out);
    }
    let ex = export(i)?;
    let Some((c, _)) = ffi::counts(&ex) else {
        return Ok(build_failure(&ex));
    };
    let Some(op) = resolve(&i.op, c) else {
        return Ok(no_entity());
    };
    if let Some(refused) = refused_input(&op) {
        return Ok(refused);
    }
    let (code, args) = op_args(&op)?;
    let mut input = brep_block(i)?;
    input.extend(args);
    let path = path_of(i.brep.as_ref())?;
    let out = ffi::eval(code, &path, &input).map_err(|e| harness(format!("c++: {e}")))?;
    let body = out.get(1..).ok_or_else(|| harness("c++: empty output"))?;
    if matches!(op, OpIn::CornerBlend { .. }) && body.first() == Some(&HANG34_GUARD) {
        return Ok(Output::rejected(
            "HANG-39: a tangency tolerance below 1e-20 where upstream reaches the \
             cross-tangent approximation (tens of seconds or more): nothing runs on either side",
        ));
    }
    Ok(guarded_output(body.to_vec()))
}

/// The output of an op: a `-2` status is the shim's guard on an input upstream would
/// dereference null on (the C++ was not run), reported as an error on both sides so
/// the case counts as `both_errored`, not as a value comparison.
fn guarded_output(o: Vec<f64>) -> Output {
    if o.first() == Some(&NULL_DEREF) {
        Output::err("null_dereference_guard")
    } else {
        Output::new().values("out", o)
    }
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
        EntityRef::Edge(x) => (16016, 8, x.index()),
        EntityRef::Vertexuse(x) => (16006, 9, x.index()),
        EntityRef::Vertex(x) => (16021, 10, x.index()),
    };
    ObjectRef {
        type_code,
        key: ObjectKey(1 + (kind << 32) + u64::from(i)),
    }
}

/// The identity of a face's surface / an edge's curve (attributed on their own
/// upstream; the Rust collections look them up under their face / edge).
fn surface_key(f: FaceId) -> ObjectKey {
    ObjectKey(1 + (11u64 << 32) + u64::from(f.index()))
}
fn curve_key(e: EdgeId) -> ObjectKey {
    ObjectKey(1 + (12u64 << 32) + u64::from(e.index()))
}

/// The entity of a resolved target (`None` for null or an unknown type).
fn entity(t: [i64; 2]) -> Option<EntityRef> {
    let [ty, i] = t;
    if i < 0 {
        return None;
    }
    let i = i as u32;
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

/// The traversal index of an entity.
fn index_of(e: EntityRef) -> i64 {
    i64::from(match e {
        EntityRef::Brep(x) => x.index(),
        EntityRef::Region(x) => x.index(),
        EntityRef::Shell(x) => x.index(),
        EntityRef::Faceuse(x) => x.index(),
        EntityRef::Face(x) => x.index(),
        EntityRef::Loopuse(x) => x.index(),
        EntityRef::Loop(x) => x.index(),
        EntityRef::Edgeuse(x) => x.index(),
        EntityRef::Edge(x) => x.index(),
        EntityRef::Vertexuse(x) => x.index(),
        EntityRef::Vertex(x) => x.index(),
    })
}

/// `SM_ERR_INVALID_INPUT`.
const SM_ERR_INVALID_INPUT: f64 = 1007.0;

/// The `SmStatus` of a Rust error: an upstream code as is; `InvalidInput` raised by the
/// unit itself marks a null dereference upstream (`-2`), from a callee it is that
/// callee's `SM_ERR_INVALID_INPUT`.
fn status_of(e: &SolidError) -> f64 {
    match e {
        SolidError::Upstream { code, .. } => f64::from(*code),
        SolidError::InvalidInput { operation, .. } if *operation == "topology::brep::p1" => {
            NULL_DEREF
        }
        SolidError::InvalidInput { .. } => SM_ERR_INVALID_INPUT,
        _ => -9.0,
    }
}

/// The `SmMarkType` of a resolved mark code (`SM_MT_MARK` 1, `MARK2` 2, `MARK3` 4,
/// `MARKIO` 8, `MARKASSERT` 16); any other code is a harness error (lesson 22).
fn mark_type(code: u8) -> Result<MarkType, HarnessError> {
    match code {
        1 => Ok(MarkType::Mark),
        2 => Ok(MarkType::Mark2),
        4 => Ok(MarkType::Mark3),
        8 => Ok(MarkType::MarkIo),
        16 => Ok(MarkType::MarkAssert),
        other => Err(harness(format!("unknown mark code {other}"))),
    }
}

/// The stand-in UV trim curve of `Redistribute` (upstream only tests it for `NULL`):
/// the unit line of the shim.
fn stand_in_uv_curve() -> Curve {
    Curve::from(solidcore::curve::line::Line::from_points(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        2,
    ))
}

fn plane(k: f64) -> Surface {
    Surface::from(Plane::from_normal(
        Vec3::new(0.0, 0.0, k),
        Vec3::new(0.0, 0.0, 1.0),
    ))
}

/// Attaches a fresh attribute `id` to each target (as the shim), returning the store.
fn attach(targets: &[[i64; 2]], id: i64) -> Result<AttributeStore, SolidError> {
    let mut store = AttributeStore::default();
    if id < 0 {
        return Ok(store);
    }
    for t in targets {
        let key = match (t[0], t[1]) {
            (_, i) if i < 0 => continue,
            (ffi::SURFACE_TYPE, i) => surface_key(FaceId::from_index(i as u32)),
            (ffi::CURVE_TYPE, i) => curve_key(EdgeId::from_index(i as u32)),
            _ => match entity(*t) {
                Some(e) => key_of(e).key,
                None => continue,
            },
        };
        let h = store.insert(Attribute::new(id as u64, Behavior::Copy))?;
        store.add_attribute(key, h)?;
    }
    Ok(store)
}

/// The deferral of an op evaluating inexact geometry: `faces` / `edges` name what it
/// touches (`None`: every face / edge of the B-rep).
fn inexact(
    types: &ImportTypes,
    faces: Option<&[FaceId]>,
    edges: Option<&[EdgeId]>,
) -> Option<Output> {
    let surface_types: Vec<i64> = match faces {
        None => (0..types.face_surface_types.len())
            .filter_map(|f| types.inexact_surface(f))
            .collect(),
        Some(fs) => fs
            .iter()
            .filter_map(|f| types.inexact_surface(f.index() as usize))
            .collect(),
    };
    for st in surface_types {
        if !EXACT_SURFACES.contains(&st) {
            // a harness limitation, not an unported unit: the shared export of
            // `topology.edgeuse_p1` hands these surfaces over as their B-spline part
            let symbol = match st {
                15012 | 15013 => "SmCone / SmCylinder face surface exported as its B-spline part",
                15014 => "SmSphere face surface exported as its B-spline part",
                15015 => "SmTorus face surface exported as its B-spline part",
                15030 => "SmSurfOfExtrusion face surface exported as its B-spline part",
                15020 => "SmSurfOfRevolution face surface exported as its B-spline part",
                _ => "face surface of a kind the shared export flattens to a B-spline",
            };
            return Some(Output::deferred(HARNESS_ANALYTIC_IMPORT, symbol));
        }
    }
    let curve_types: Vec<i64> = match edges {
        None => (0..types.edge_curve_types.len())
            .filter_map(|e| types.inexact_curve(e))
            .collect(),
        Some(es) => es
            .iter()
            .filter_map(|e| types.inexact_curve(e.index() as usize))
            .collect(),
    };
    for t in curve_types {
        if t >= 0 && !EXACT_CURVES.contains(&t) {
            return Some(Output::deferred(
                HARNESS_ANALYTIC_IMPORT,
                "edge curve (SmEllipse / SmCircle / other analytic kind) exported as its B-spline part",
            ));
        }
    }
    None
}

fn ids<I: solidcore::topology::ArenaId>(v: &[I]) -> Vec<f64> {
    let mut out = vec![v.len() as f64];
    out.extend(v.iter().map(|x| f64::from(x.index())));
    out
}

/// A curve's fingerprint, as the shim's `put_curve_id`: `-1` (none), else `1 t0 t1
/// P(t0) P(mid) P(t1)`.
fn curve_id(k: &Kernel, c: Option<&Curve>, out: &mut Vec<f64>) {
    let Some(c) = c else {
        out.push(-1.0);
        return;
    };
    out.push(1.0);
    let ivl = c.natural_interval();
    out.extend([ivl.min(), ivl.max()]);
    for t in [ivl.min(), ivl.mid(), ivl.max()] {
        let p = c.evaluate_point(k, t).unwrap_or(Vec3::UNDEF);
        out.extend([p.x, p.y, p.z]);
    }
}

/// The state of an edge use, as the shim's `put_eu_state`.
fn eu_state(k: &Kernel, b: &Brep, eu: EdgeuseId, out: &mut Vec<f64>) -> Result<(), SolidError> {
    out.push(1.0);
    let r = b.edgeuse(eu)?;
    curve_id(k, r.uv_trim_curve.as_ref(), out);
    out.push(if r.uv_trim_curve.is_some() {
        f64::from(eu.index())
    } else {
        -1.0
    });
    match r.drop_curve_fail.as_deref() {
        Some(f) => out.extend([1.0, f64::from(f.fail_status), flag(f.brep.is_some())]),
        None => out.extend([0.0, 0.0, 0.0]),
    }
    match r.vertexuse {
        Some(vu) => out.push(flag(b.vertexuse(vu)?.vertex_face_gap.is_init())),
        None => out.push(-1.0),
    }
    Ok(())
}

/// A surface block, as the shim's `put_surface` (B-spline surfaces and planes; an
/// `SmSurfOfRevolution` is an `SmBSplineSurface`, written as its B-spline part).
fn put_surface(o: &mut Vec<f64>, s: &Surface) -> Result<(), HarnessError> {
    let base = match s.kind() {
        SurfaceKind::BSpline(bs) => Some(bs),
        SurfaceKind::SurfOfRevolution(r) => Some(r.base()),
        SurfaceKind::Sphere(r) => Some(r.base().base()),
        SurfaceKind::Torus(r) => Some(r.base().base()),
        SurfaceKind::SurfOfExtrusion(r) => Some(r.base()),
        SurfaceKind::Cone(r) => Some(r.rev().base()),
        _ => None,
    };
    match (s.kind(), base) {
        (SurfaceKind::Plane(p), _) => {
            o.push(1.0);
            let pos = p.position();
            for v in [pos.origin(), pos.x_axis(), pos.y_axis()] {
                o.extend([v.x, v.y, v.z]);
            }
            let sc = p.uv_scale();
            o.extend([sc.x, sc.y]);
            let d = p.step_uv_domain();
            o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
        }
        (_, Some(bs)) => {
            let nurb = bs
                .gw_nurb()
                .ok_or_else(|| harness("blend surface without NURBS data"))?;
            let (p, q) = nurb.degrees();
            o.extend([
                0.0,
                flag(bs.is_out_of_bounds_enabled()),
                bs.form() as u64 as f64,
                nurb.n() as f64,
                nurb.m() as f64,
                p as f64,
                q as f64,
                (nurb.u_knots().len() - 1) as f64,
                (nurb.v_knots().len() - 1) as f64,
            ]);
            for row in nurb.cpts() {
                for pt in row {
                    o.extend([pt.x, pt.y, pt.z, pt.w]);
                }
            }
            o.extend_from_slice(nurb.u_knots());
            o.extend_from_slice(nurb.v_knots());
        }
        _ => o.push(-2.0),
    }
    Ok(())
}

/// The code of an `SmOrientType`, as the shared import reads it.
fn orient_code(o: OrientType) -> f64 {
    match o {
        OrientType::Unknown => 0.0,
        OrientType::Same => 1.0,
        OrientType::Opposite => 2.0,
    }
}

/// An optional id as the shim's `Graph::find`: the index, `-1` for none.
fn idx<I: solidcore::topology::ArenaId>(id: Option<I>) -> f64 {
    id.map_or(-1.0, |i| f64::from(i.index()))
}

/// The healed B-rep, as the shim's `put_healed`: the `Export` extras (infinite region,
/// editing flag, size estimate and ratio, list head, the vertex use lists, the face,
/// edge and vertex list orders), the ten counts, then per record the fields the Rust
/// B-rep holds (see the shim), in arena order. The import numbers the arenas in the
/// shim's graph order, and the shim re-walks the healed B-rep, so the two agree while
/// the heal leaves the topology lists as they are; a step that reorders or adds records
/// shows up as a disagreement, never as a silent pass.
fn put_healed(k: &Kernel, b: &Brep, types: &ImportTypes, o: &mut Vec<f64>) -> Result<(), Fail> {
    o.push(idx(b.infinite_region()));
    o.push(flag(b.editing_enabled()));
    o.push(b.this_model_size_estimate(k, false));
    o.push(b.this_large_small_size_ratio(k, false));
    o.push(flag(b.region_list_head() == b.infinite_region()));
    o.push(b.vertices().len() as f64);
    for (_, v) in b.vertices().iter() {
        o.extend(ids(&v.vertexuses));
    }
    o.extend(ids(&bp1::faces(b, None)?));
    o.extend(ids(&bp1::edges(b, None)?));
    o.extend(ids(&bp1::vertices(b, None)?));
    for n in [
        b.regions().len(),
        b.shells().len(),
        b.faceuses().len(),
        b.faces().len(),
        b.loopuses().len(),
        b.loops().len(),
        b.edgeuses().len(),
        b.edges().len(),
        b.vertexuses().len(),
        b.vertices().len(),
    ] {
        o.push(n as f64);
    }
    for (_, r) in b.regions().iter() {
        o.push(flag(r.in_brep));
    }
    for (_, s) in b.shells().iter() {
        o.push(idx(s.region));
    }
    for (_, fu) in b.faceuses().iter() {
        o.extend([
            idx(fu.shell),
            idx(fu.face),
            idx(fu.mate),
            orient_code(fu.orientation),
        ]);
        o.extend(ids(&fu.loopuses));
    }
    for (id, f) in b.faces().iter() {
        match f.surface.as_ref() {
            Some(s) => put_surface(o, s)?,
            None => o.push(-1.0),
        }
        let d = &f.uv_domain;
        o.extend([d.min().x, d.min().y, d.max().x, d.max().y, f.tolerance]);
        o.push(
            types
                .face_surface_types
                .get(id.index() as usize)
                .map_or(-1.0, |&t| t as f64),
        );
    }
    for (_, lu) in b.loopuses().iter() {
        o.extend([idx(lu.faceuse), idx(lu.loop_), idx(lu.mate)]);
        match lu.start {
            LoopuseStart::Edgeuse(eu) => o.extend([1.0, idx(eu)]),
            LoopuseStart::Vertexuse(vu) => o.extend([2.0, idx(vu)]),
            LoopuseStart::Unknown => o.extend([0.0, -1.0]),
        }
        o.push(orient_code(lu.orientation));
    }
    for (_, l) in b.loops().iter() {
        o.push(idx(l.loopuse));
    }
    for (_, eu) in b.edgeuses().iter() {
        o.push(idx(eu.edge));
        match eu.owner {
            EdgeuseOwner::Loopuse(lu) => o.extend([1.0, idx(lu)]),
            EdgeuseOwner::Shell(s) => o.extend([2.0, idx(s)]),
            EdgeuseOwner::Unknown => o.extend([0.0, -1.0]),
        }
        o.extend([
            idx(eu.cw),
            idx(eu.ccw),
            idx(eu.vertexuse),
            orient_code(eu.orientation),
        ]);
        curve_id(k, eu.uv_trim_curve.as_ref(), o);
        match eu.drop_curve_fail.as_deref() {
            Some(f) => o.extend([1.0, f64::from(f.fail_status), flag(f.brep.is_some())]),
            None => o.extend([0.0, 0.0, 0.0]),
        }
        o.extend([
            flag(eu.max_edge_face_gap.is_init()),
            flag(eu.edge_ccw_edge_gap.is_init()),
        ]);
    }
    for (id, e) in b.edges().iter() {
        o.extend([e.tolerance, e.interval.min(), e.interval.max()]);
        o.extend(ids(&e.edgeuses));
        o.push(
            types
                .edge_curve_types
                .get(id.index() as usize)
                .map_or(-1.0, |&t| t as f64),
        );
        curve_id(k, e.curve.as_ref(), o);
    }
    for (_, vu) in b.vertexuses().iter() {
        o.push(idx(vu.vertex));
        match vu.owner {
            VertexuseOwner::Edgeuse(eu) => o.extend([1.0, idx(eu)]),
            VertexuseOwner::Loopuse(lu) => o.extend([2.0, idx(lu)]),
            VertexuseOwner::Shell(s) => o.extend([3.0, idx(s)]),
            VertexuseOwner::Unknown => o.extend([0.0, -1.0]),
        }
        o.push(flag(vu.vertex_face_gap.is_init()));
    }
    for (_, v) in b.vertices().iter() {
        o.extend([v.point.x, v.point.y, v.point.z, v.tolerance]);
    }
    o.extend([b.tolerance(), flag(b.validate_only())]);
    Ok(())
}

fn run_rust_op(
    k: &mut Kernel,
    b: &mut Brep,
    types: &ImportTypes,
    op: &OpIn,
) -> Result<Result<Vec<f64>, Output>, Fail> {
    let mut o: Vec<f64> = Vec::new();
    match op {
        OpIn::BrepIntersect2 { .. } => {
            return Err(harness("cross-body observation bypasses legacy graph import").into());
        }
        OpIn::Lists {
            which,
            attr,
            targets,
        } => {
            let store = attach(targets, *attr)?;
            let key = |e: EntityRef| key_of(e).key;
            let filter = (*attr >= 0).then_some(AttributeFilter {
                attributes: &store,
                attribute_id: *attr as u64,
                key_of: &key,
            });
            let f = filter.as_ref();
            // GetSurfaces / GetCurves test the geometry's own attributes
            let geometry_filter = (*attr >= 0).then_some(bp1::GeometryAttributeFilter {
                attributes: &store,
                attribute_id: *attr as u64,
                surface_key_of: &surface_key,
                curve_key_of: &curve_key,
            });
            let gf = geometry_filter.as_ref();
            match which {
                5 | 6 => {
                    if let Some(d) = inexact(types, None, None) {
                        return Ok(Err(d));
                    }
                }
                7 if b.faces().iter().any(|(_, r)| r.surface.is_none()) => {
                    return Ok(Ok(vec![NULL_DEREF]));
                }
                8 if b.edges().iter().any(|(_, r)| r.curve.is_none()) => {
                    return Ok(Ok(vec![NULL_DEREF]));
                }
                _ => {}
            }
            o.push(0.0);
            let list = match which {
                0 => ids(&bp1::regions(b, f)?),
                1 => ids(&bp1::faces(b, f)?),
                2 => ids(&bp1::edges(b, f)?),
                3 => ids(&bp1::vertices(b, f)?),
                4 => ids(&bp1::wire_edges(b, f)?),
                5 => ids(&bp1::topological_edges(k, b)?),
                6 => ids(&bp1::topological_vertices(k, b)?),
                7 => {
                    // the surfaces' owners (`GetOwner()`), as face indices
                    let owners: Vec<f64> = bp1::surfaces(b, gf)?
                        .iter()
                        .map(|s| s.owner().map_or(-1.0, |o| o.key.0 as f64))
                        .collect();
                    let mut v = vec![owners.len() as f64];
                    v.extend(owners);
                    v
                }
                8 => {
                    // the curves' owners (`GetOwner()`), as edge indices
                    let owners: Vec<f64> = bp1::curves(b, gf)?
                        .iter()
                        .map(|c| c.owner().map_or(-1.0, |o| o.key.0 as f64))
                        .collect();
                    let mut v = vec![owners.len() as f64];
                    v.extend(owners);
                    v
                }
                _ => ids(&bp1::shells(k, b, f)?),
            };
            o.extend(list);
        }
        OpIn::Query { ty, index } => {
            let t = entity([*ty, *index]);
            o.push(bp1::num_faces(b) as f64);
            o.push(flag(bp1::is_live_topology_member(b, t)));
            o.push(b.tolerance());
            o.push(b.this_model_size_estimate(k, false));
            o.push(b.this_model_size_estimate(k, true));
            o.push(b.this_large_small_size_ratio(k, false));
            o.push(b.this_large_small_size_ratio(k, true));
            o.push(b.infinite_region().map_or(-1.0, |r| f64::from(r.index())));
            o.push(bp1::TYPE as f64);
            for t in KIND_TYPES {
                o.push(flag(bp1::is_kind_of(t)));
            }
            o.push(bp1::TYPE_STRING.len() as f64);
            o.push(bp1::CLASS_STRING.len() as f64);
            o.push(flag(b.editing_enabled()));
            o.push(flag(b.validate_only()));
        }
        OpIn::SetSize { kind, value } => {
            if *kind == 0 {
                let r = b.set_this_model_size_estimate(k, *value);
                o.push(r);
                o.push(b.tolerance());
                o.push(b.this_model_size_estimate(k, false));
                o.push(b.this_model_size_estimate(k, true));
                o.push(k.context.this_model_size_estimate(k));
                o.push(k.context.this_zone_tol_3d());
            } else {
                b.set_this_large_small_size_ratio(k, *value);
                o.push(b.this_large_small_size_ratio(k, false));
                o.push(b.this_large_small_size_ratio(k, true));
                // SmContext::GetThisLargeSmallSizeRatio
                let r = k.context.stored_large_small_size_ratio();
                o.push(if r == tol::USE_DEFAULT {
                    k.tolerances.large_small_size_ratio
                } else {
                    r
                });
            }
        }
        OpIn::SetInf {
            set,
            mode,
            reset,
            flag: void_flag,
            unsure,
        } => {
            // SmRegion::SetIsVoid(UNSURE) on the chosen region first
            if *unsure >= 0 {
                b.region_mut(RegionId::from_index(*unsure as u32))?.is_void = 2;
            }
            if *set != -2 {
                b.set_infinite_region((*set >= 0).then(|| RegionId::from_index(*set as u32)));
            }
            // SmSetRegionVoidType: SM_VS_NESTEDSOLID 0, _INFINITEREGION_ONLY 1, _SWAP 2 (the FFI
            // refuses anything else; lesson 22: never default an unknown code)
            let void_type = match void_flag {
                0 => bp1::SetRegionVoidType::NestedSolid,
                1 => bp1::SetRegionVoidType::InfiniteRegionOnly,
                2 => bp1::SetRegionVoidType::InfiniteRegionSwap,
                other => {
                    return Err(Fail::Harness(harness(format!(
                        "unknown SmSetRegionVoidType code {other}"
                    ))));
                }
            };
            let r = match mode {
                1 => bp1::reset_infinite_region(
                    k,
                    b,
                    (*reset >= 0).then(|| RegionId::from_index(*reset as u32)),
                    void_type,
                ),
                2 => bp1::set_region_is_void_flags_for_nested_solids(k, b),
                _ => Ok(()),
            };
            let status = match r {
                Ok(()) => 0.0,
                Err(e) if e.is_unported() => return Err(e.into()),
                Err(e) => status_of(&e),
            };
            o.push(status);
            // the shim stops at the guard where upstream dereferences the null old
            // infinite region (the swap flag); otherwise it reports the state
            if status != NULL_DEREF {
                o.push(b.infinite_region().map_or(-1.0, |r| f64::from(r.index())));
                o.extend(ids(&bp1::regions(b, None)?));
            }
            // a successful mode 1/2: every region's raw IsVoid flag, in the export's
            // region order (the shim's traversal registry)
            if *mode != 0 && status == 0.0 {
                let flags: Vec<f64> = b
                    .regions()
                    .iter()
                    .map(|(_, r)| f64::from(r.is_void))
                    .collect();
                o.push(flags.len() as f64);
                o.extend(flags);
            }
        }
        OpIn::Degenerate { tol } => {
            o.push(flag(bp1::is_degenerate(k, b, *tol)?));
        }
        OpIn::AddDeleted { n } => {
            for i in 0..*n {
                b.add_deleted_surface(plane(f64::from(i)));
            }
            o.push(b.deleted_surfaces().len() as f64);
        }
        OpIn::Vote { regions } => {
            let mut rs: Vec<Option<RegionId>> = Vec::new();
            let mut votes: Vec<u64> = Vec::new();
            for &r in regions {
                // a null vote (-1: the shim votes with a null pointer) is a region of its own
                bp1::vote_for_region(
                    &mut rs,
                    &mut votes,
                    (r >= 0).then(|| RegionId::from_index(r as u32)),
                );
            }
            o.push(rs.len() as f64);
            o.extend(rs.iter().map(|r| r.map_or(-1.0, |r| f64::from(r.index()))));
            o.push(votes.len() as f64);
            o.extend(votes.iter().map(|&v| v as f64));
            o.push(bp1::find_highest_vote(&rs, &votes)?.map_or(-1.0, |r| f64::from(r.index())));
        }
        OpIn::ObjMap { sets, gets } => {
            // the copy maps are only asked for stored keys (UB-98, as the shim)
            let mut map = bp1::ObjectMap::new();
            let mut copy_map = CopyBrepMap::<EntityRef>::new();
            let mut rev_map = CopyBrepMap::<EntityRef>::new();
            let mut copy_keys = BTreeSet::new();
            let mut rev_keys = BTreeSet::new();
            for s in sets {
                let Some(orig) = entity(s.orig) else {
                    continue;
                };
                let copy = entity(s.copy);
                bp1::set_object_map(
                    orig,
                    copy,
                    &mut map,
                    s.copy_map.then_some(&mut copy_map),
                    s.reverse.then_some(&mut rev_map),
                );
                if let Some(c) = copy {
                    if s.copy_map {
                        copy_keys.insert(c);
                    }
                    if s.reverse {
                        rev_keys.insert(orig);
                    }
                }
            }
            let mut v: Vec<i64> = Vec::new();
            for g in gets {
                let from = entity(*g);
                let to = from.and_then(|f| bp1::get_object_map(f, &map));
                v.push(to.map_or(-1, |t| key_of(t).type_code));
                v.push(to.map_or(-1, index_of));
                let c = match from {
                    Some(f) if copy_keys.contains(&f) => copy_map.get_at(Some(f))?,
                    _ => None,
                };
                v.push(c.map_or(-1, index_of));
                let r = match from {
                    Some(f) if rev_keys.contains(&f) => rev_map.get_at(Some(f))?,
                    _ => None,
                };
                v.push(r.map_or(-1, index_of));
            }
            o.extend(list_of(&v));
        }
        OpIn::AddAttrs {
            target,
            ids: attr_ids,
            indexed,
        } => {
            let Some(t) = entity(*target) else {
                // upstream dereferences the null object once per index; with no index
                // it returns SM_SUCCESS (and the object has no attributes to list)
                return Ok(Ok(if indexed.is_empty() {
                    vec![0.0, 0.0]
                } else {
                    vec![NULL_DEREF]
                }));
            };
            let mut store = AttributeStore::default();
            let mut handles = Vec::new();
            for &id in attr_ids {
                handles.push(store.insert(Attribute::new(id as u64, Behavior::Copy))?);
            }
            let indexed: Vec<usize> = indexed.iter().map(|&i| i as usize).collect();
            let key = key_of(t).key;
            match bp1::add_attributes(&mut store, key, &indexed, &handles) {
                Ok(()) => o.push(0.0),
                Err(e) => o.push(status_of(&e)),
            }
            let got: Vec<i64> = store
                .attributes(key)
                .iter()
                .filter_map(|h| store.get(*h).map(|a| a.attribute_id() as i64))
                .collect();
            o.extend(list_of(&got));
        }
        OpIn::ReflectEu { edge } => {
            let e = EdgeId::from_index(*edge as u32);
            match bp1::reverse_reflection_edgeuses(b, e) {
                Ok(()) => {
                    o.push(0.0);
                    o.extend(ids(&b.edge(e)?.edgeuses));
                }
                Err(e) => o.push(status_of(&e)),
            }
        }
        OpIn::ReflectShell { face } => {
            let f = FaceId::from_index(*face as u32);
            match bp1::swap_reflection_shells(b, f) {
                Ok(()) => {
                    o.push(0.0);
                    let fu1 = b.face(f)?.faceuse.ok_or_else(|| harness("no face use"))?;
                    let fu2 = b.faceuse(fu1)?.mate.ok_or_else(|| harness("no mate"))?;
                    for fu in [fu1, fu2] {
                        o.push(b.faceuse(fu)?.shell.map_or(-1.0, |s| f64::from(s.index())));
                    }
                    let counts: Vec<i64> = b
                        .shells()
                        .ids()
                        .map(|s| {
                            let n = b
                                .faceuses()
                                .iter()
                                .filter(|(_, r)| r.shell == Some(s))
                                .count();
                            if n == 0 {
                                -1
                            } else {
                                n as i64
                            }
                        })
                        .collect();
                    o.extend(list_of(&counts));
                    for (_, shell) in b.shells().iter() {
                        let members = shell.get_faceuses()?;
                        o.extend(ids(&members));
                        for member in members {
                            let (next, previous) =
                                solidcore::topology::shell::faceuse_sibling_links(b, member)?;
                            o.extend(
                                [next, previous]
                                    .map(|id| id.map_or(-1.0, |id| f64::from(id.index()))),
                            );
                        }
                    }
                }
                Err(e) => o.push(status_of(&e)),
            }
        }
        OpIn::RmLoopVu { vu } => {
            let id = VertexuseId::from_index(*vu as u32);
            let v = b.vertexuse(id)?.vertex;
            let r = bp1::rm_and_del_old_loop_vertices(b, id);
            let size = match v {
                Some(v) => b.vertex(v)?.vertexuses.len() as f64,
                None => -1.0,
            };
            match r {
                Ok(()) => {
                    o.extend([0.0, size]);
                    if let Some(v) = v {
                        o.extend(ids(&b.vertex(v)?.vertexuses));
                    }
                }
                Err(e) => {
                    let st = status_of(&e);
                    o.push(st);
                    if st != NULL_DEREF {
                        o.push(size);
                    }
                }
            }
        }
        OpIn::MarkLoops { loop_, level } => {
            let mut loops: Vec<LoopId> = Vec::new();
            match bp1::mark_loops_to_delete(
                b,
                &mut loops,
                LoopId::from_index(*loop_ as u32),
                *level,
            ) {
                Ok(()) => {
                    o.push(0.0);
                    o.extend(ids(&loops));
                }
                Err(e) => {
                    let st = status_of(&e);
                    o.push(st);
                    if st != NULL_DEREF {
                        o.extend(ids(&loops));
                    }
                }
            }
        }
        OpIn::SwapFaceUv {
            face,
            mark,
            premark,
        } => {
            let f = FaceId::from_index(*face as u32);
            let mark = mark_type(*mark)?;
            let mut marks = Marks::default();
            if *premark {
                marks.mark(k, Entity::Face(f), mark);
            }
            let swaps = !marks.is_marked(k, Entity::Face(f), mark);
            match bp1::swap_face_uv(k, b, &mut marks, f, mark) {
                Err(e) if !e.is_unported() => return Ok(Ok(vec![status_of(&e)])),
                r => r?,
            }
            // a swap transforms the surface and mirrors the trim curves: compared only
            // where the import is exact
            if swaps {
                if let Some(d) = inexact(types, None, None) {
                    return Ok(Err(d));
                }
            }
            o.push(0.0);
            o.push(flag(marks.is_marked(k, Entity::Face(f), mark)));
            let d = b.face(f)?.uv_domain;
            o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
            if swaps {
                put_healed(k, b, types, &mut o)?;
            }
        }
        OpIn::CheckUv { eu } => {
            let e = EdgeuseId::from_index(*eu as u32);
            if b.edgeuse(e)?.uv_trim_curve.is_some() {
                let face = solidcore::topology::edgeuse::p1::face(b, e)?;
                let edge = b.edgeuse(e)?.edge;
                let fs: Vec<FaceId> = face.into_iter().collect();
                let es: Vec<EdgeId> = edge.into_iter().collect();
                if let Some(d) = inexact(types, Some(&fs), Some(&es)) {
                    return Ok(Err(d));
                }
            }
            o.push(match bp1::check_uv_trim_curve(k, b, e) {
                Ok(()) => 0.0,
                Err(e) if e.is_unported() => return Err(e.into()),
                Err(e) => status_of(&e),
            });
        }
        OpIn::LockBox => {
            bp1::LockBrepBox::new(k, b)?;
            o.push(0.0);
        }
        OpIn::FindSide { curve, p, dir, tol } => {
            let c = Curve::from(bspline_of(curve)?);
            match bp1::find_side_of_curve(k, &c, Vec3::from_array(*p), Vec3::from_array(*dir), *tol)
            {
                Ok(right) => o.extend([0.0, flag(right)]),
                Err(e) if e.is_unported() => return Err(e.into()),
                Err(e) => o.extend([status_of(&e), 2.0]), // UNSURE
            }
        }
        OpIn::FindSector {
            face,
            guess,
            p,
            dir,
        } => {
            let f = FaceId::from_index(*face as u32);
            if let Some(d) = inexact(types, Some(&[f]), Some(&[])) {
                return Ok(Err(d));
            }
            let guess = guess.map(|[u, v]| Point2::new(u, v));
            match bp1::find_sector_of_face(
                k,
                b,
                f,
                guess,
                Vec3::from_array(*p),
                Vec3::from_array(*dir),
            ) {
                Ok((dist, region)) => {
                    o.extend([0.0, dist, region.map_or(-1.0, |r| f64::from(r.index()))]);
                }
                Err(e) if e.is_unported() => return Err(e.into()),
                Err(e) => o.push(status_of(&e)),
            }
        }
        OpIn::UpdateIvls { vertex } => {
            let v = VertexId::from_index(*vertex as u32);
            let edges = solidcore::topology::vertex::edges(b, v, None)?;
            if edges
                .iter()
                .any(|&e| b.edge(e).map_or(true, |r| r.curve.is_none()))
            {
                return Ok(Ok(vec![NULL_DEREF]));
            }
            if let Some(d) = inexact(types, Some(&[]), Some(&edges)) {
                return Ok(Err(d));
            }
            let mut records = BTreeMap::new();
            let mut attrs = AttributeStore::default();
            let mut env = solidcore::topology::edge::NotifyEnv {
                records: &mut records,
                attributes: &mut attrs,
                context: None,
                key_of: &key_of,
            };
            let r = bp1::update_vertex_edge_intervals(k, b, &mut env, v);
            match r {
                Ok(()) => o.push(0.0),
                Err(e) if e.is_unported() => return Err(e.into()),
                Err(e) => o.push(status_of(&e)),
            }
            o.push(b.vertex(v)?.tolerance);
            o.push(edges.len() as f64);
            for e in edges {
                let r = b.edge(e)?;
                o.extend([r.interval.min(), r.interval.max(), r.tolerance]);
            }
        }
        OpIn::Redistribute {
            edge,
            has_uv,
            seam,
            dom,
        } => {
            let e = EdgeId::from_index(*edge as u32);
            let seam = SurfParamType::from_code(u64::from(*seam))
                .ok_or_else(|| harness(format!("seam code {seam} outside SM_SP_U..=SM_SP_VMAX")))?;
            // the trim curve argument is only tested for NULL: a stand-in on both sides
            let uv: Option<Curve> = has_uv.then(stand_in_uv_curve);
            let d = Extent2d::from_bounds(dom[0], dom[1], dom[2], dom[3]);
            match bp1::redistribute_seam_uv_trim_curves(k, b, e, uv.as_ref(), seam, &d) {
                Ok(swapped) => o.extend([0.0, flag(swapped)]),
                Err(err) if err.is_unported() => return Err(err.into()),
                Err(err) => o.push(status_of(&err)),
            }
        }
        OpIn::SplitUv {
            eu_orig,
            eu_new,
            p,
            t,
            scaled_zero,
            orient,
            for_tess,
        } => {
            let eo = EdgeuseId::from_index(*eu_orig as u32);
            let en = EdgeuseId::from_index(*eu_new as u32);
            if b.edgeuse(eo)?.uv_trim_curve.is_some() {
                let Some(e) = b.edgeuse(eo)?.edge else {
                    return Ok(Ok(vec![NULL_DEREF]));
                };
                let face = solidcore::topology::edgeuse::p1::face(b, eo)?;
                let fs: Vec<FaceId> = face.into_iter().collect();
                if let Some(d) = inexact(types, Some(&fs), Some(&[e])) {
                    return Ok(Err(d));
                }
            }
            // SmOrientType: SM_OT_UNKNOWN = 0, SM_OT_SAME = 1, SM_OT_OPPOSITE = 2 (the FFI
            // refuses anything else; lesson 22: never default an unknown code)
            let orient = match orient {
                0 => OrientType::Unknown,
                1 => OrientType::Same,
                2 => OrientType::Opposite,
                other => {
                    return Err(Fail::Harness(harness(format!(
                        "unknown orientation code {other}"
                    ))));
                }
            };
            let r = bp1::make_vertex_split_uv_trim_curve(
                k,
                b,
                eo,
                en,
                Vec3::from_array(*p),
                *t,
                *scaled_zero,
                orient,
                *for_tess,
            );
            match r {
                Ok(()) => o.push(0.0),
                Err(e) if e.is_unported() => return Err(e.into()),
                Err(e) => o.push(status_of(&e)),
            }
            eu_state(k, b, eo, &mut o)?;
            eu_state(k, b, en, &mut o)?;
            for eu in [eo, en] {
                o.push(match b.edgeuse(eu)?.edge {
                    Some(e) => b.edge(e)?.tolerance,
                    None => -1.0,
                });
            }
        }
        OpIn::BrepIntersect {
            tol,
            ang,
            want_points,
        } => {
            // the B-rep against itself, as the shim's `brep->BrepIntersect(ctx, brep, ..)`
            let (curves, points) = bp1::brep_intersect_self(k, b, *tol, *ang, *want_points)?;
            o.extend([
                0.0,
                curves.len() as f64,
                points.map_or(-1.0, |p| p.len() as f64),
            ]);
        }
        OpIn::CornerBlend {
            eus,
            tangent,
            tol,
            ang,
        } => {
            let ids: Vec<EdgeuseId> = eus
                .iter()
                .map(|&i| EdgeuseId::from_index(i as u32))
                .collect();
            match bp1::create_corner_blend(k, b, &ids, tangent.as_deref(), *tol, *ang) {
                Ok(s) => {
                    o.extend([0.0, s.len() as f64]);
                    for blend in s {
                        put_surface(&mut o, &Surface::from(blend))?;
                    }
                }
                Err(e) if e.is_unported() => return Err(e.into()),
                // UB-195: an empty rational-curve drop is where upstream reads sUVCurves[0]
                // of an empty array; the port's InvalidInput there is that null dereference
                Err(SolidError::InvalidInput {
                    operation: "nurbs::b_spline_surface_n_lib::p1::create_non_rational_curve",
                    ..
                }) => o.extend([NULL_DEREF, 0.0]),
                // upstream resets the output array before failing
                Err(e) => o.extend([status_of(&e), 0.0]),
            }
        }
        OpIn::EdgeEdgeBlend {
            flag: f,
            eu1,
            eu2,
            dir,
        } => {
            let id = |i: i64| (i >= 0).then(|| EdgeuseId::from_index(i as u32));
            // a blend (flag != 0) samples both edge uses' face surfaces (DropPoint,
            // normals, directional derivatives): an analytic surface the export hands
            // over as its B-spline part takes other paths in C++ (e.g.
            // SmSurfOfRevolution::DropPointFast), so those cases are not comparable
            if *f != 0 {
                if let (Some(e1), Some(e2)) = (id(*eu1), id(*eu2)) {
                    let face = solidcore::topology::edgeuse::p1::face;
                    let has_edges = b.edgeuse(e1)?.edge.is_some() && b.edgeuse(e2)?.edge.is_some();
                    if let (true, Some(f1), Some(f2)) = (has_edges, face(b, e1)?, face(b, e2)?) {
                        if let Some(d) = inexact(types, Some(&[f1, f2]), Some(&[])) {
                            return Ok(Err(d));
                        }
                    }
                }
            }
            match bp1::create_edge_edge_blend(
                k,
                u64::from(*f),
                b,
                id(*eu1),
                None,
                id(*eu2),
                u64::from(*dir),
                None,
            ) {
                Ok(s) => {
                    o.push(0.0);
                    put_surface(&mut o, &s)?;
                }
                Err(e) if e.is_unported() => return Err(e.into()),
                Err(e) => o.push(status_of(&e)),
            }
        }
        OpIn::EdgeEdgeBlend2 {
            other,
            eu1,
            eu2,
            dir,
        } => {
            // the second B-rep in its own arena, built by the Rust kernel
            let mut b2 = match crate::own_builder::build(k, other, false, false)? {
                Some(crate::own_builder::Built::Brep(b2)) => *b2,
                Some(crate::own_builder::Built::Failed(e)) if e.is_unported() => {
                    return Err(e.into());
                }
                Some(crate::own_builder::Built::Failed(_)) => {
                    return Err(harness("the second B-rep does not build").into());
                }
                None => {
                    // an `.smb` file: imported from its own C++ export
                    let block2 = brep_block_of(Some(other), false, false)?;
                    let ex2 = ffi::eval(Op::Export, "", &block2)
                        .map_err(|e| harness(format!("c++ export of the second B-rep: {e}")))?;
                    let (_, graph_at2) = ffi::counts(&ex2)
                        .ok_or_else(|| harness("the second B-rep does not build"))?;
                    import(&ex2, graph_at2)?.0
                }
            };
            let id = |i: i64| (i >= 0).then(|| EdgeuseId::from_index(i as u32));
            match bp1::create_edge_edge_blend(
                k,
                0,
                b,
                id(*eu1),
                Some(&mut b2),
                id(*eu2),
                u64::from(*dir),
                None,
            ) {
                Ok(s) => {
                    o.push(0.0);
                    put_surface(&mut o, &s)?;
                }
                Err(e) if e.is_unported() => return Err(e.into()),
                Err(e) => o.push(status_of(&e)),
            }
        }
        OpIn::ParallelProj {
            curve,
            vec,
            dist,
            tol,
            ang,
            want_3d,
            want_uv,
            want_faces,
        } => {
            if b.faces().iter().any(|(_, r)| r.surface.is_none()) {
                return Ok(Ok(vec![NULL_DEREF]));
            }
            if let Some(d) = inexact(types, None, Some(&[])) {
                return Ok(Err(d));
            }
            let c = bspline_of(curve)?;
            let opts = bp1::ParallelProjectionOptions {
                projection_distance: *dist,
                approx_tol_3d: *tol,
                angle_tol_radians: *ang,
                subset_faces: None,
                want_3d: *want_3d,
                want_uv: *want_uv,
                want_faces: *want_faces,
            };
            match bp1::create_parallel_projection_curves(k, b, &c, Vec3::from_array(*vec), &opts) {
                Ok(r) => {
                    o.push(0.0);
                    match r.curves_3d {
                        None => o.push(-1.0),
                        Some(cs) => {
                            o.push(cs.len() as f64);
                            for c in &cs {
                                curve_id(k, Some(c), &mut o);
                            }
                        }
                    }
                    match r.uv_curves {
                        None => o.push(-1.0),
                        Some(cs) => {
                            o.push(cs.len() as f64);
                            for c in &cs {
                                curve_id(k, c.as_ref(), &mut o);
                            }
                        }
                    }
                    match r.faces_of_uv_curves {
                        None => o.push(-1.0),
                        Some(fs) => {
                            o.push(fs.len() as f64);
                            o.extend(fs.iter().map(|f| f64::from(f.index())));
                        }
                    }
                }
                Err(e) if e.is_unported() => return Err(e.into()),
                Err(e) => o.push(status_of(&e)),
            }
        }
        OpIn::Heal { healer_op, make_uv } => {
            let healer_op = HealerOpType::from_code(i64::from(*healer_op)).ok_or_else(|| {
                harness(format!(
                    "healer op {healer_op} outside SM_HO_NONE..=SM_HO_ALL (0..=26)"
                ))
            })?;
            bp1::heal_brep(k, b, healer_op, None, None, None, *make_uv)?;
            // the healer evaluates the geometry: compared only where the import is exact
            if let Some(d) = inexact(types, None, None) {
                return Ok(Err(d));
            }
            o.push(0.0);
            put_healed(k, b, types, &mut o)?;
        }
    }
    Ok(Ok(o))
}

fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    if let OpIn::BrepIntersect2 {
        other,
        tol,
        ang,
        want_points,
    } = &i.op
    {
        return run_rust_intersection(i, other, *tol, *ang, *want_points);
    }
    let mut k = Kernel::default();
    let (mut b, types, c) = match rust_brep(&mut k, i)? {
        Ok(x) => x,
        Err(o) => return Ok(o),
    };
    let Some(op) = resolve(&i.op, c) else {
        return Ok(no_entity());
    };
    if let Some(refused) = refused_input(&op) {
        return Ok(refused);
    }
    match run_rust_op(&mut k, &mut b, &types, &op) {
        Ok(Ok(o)) => Ok(guarded_output(o)),
        Ok(Err(o)) => Ok(o),
        Err(Fail::Solid(e)) => match Output::unported(&e) {
            Some(o) => Ok(o),
            None => Err(harness(format!("rust: {e}"))),
        },
        Err(Fail::Harness(h)) => Err(h),
    }
}

/// The Rust B-rep of the case, its geometry types and entity counts, or the output of
/// a failed build: shim-built sources are built by the Rust kernel
/// ([`crate::own_builder`]); only `.smb` file sources are still imported from this
/// unit's export.
fn rust_brep(
    k: &mut Kernel,
    i: &Inputs,
) -> Result<crate::own_builder::RustBrep<(Brep, ImportTypes, [usize; 10])>, HarnessError> {
    use crate::own_builder::{build, build_status, counts, Built};
    let src = i
        .brep
        .as_ref()
        .ok_or_else(|| harness("B-rep op without brep"))?;
    match build(k, src, i.validate_only, i.init_gaps)? {
        Some(Built::Brep(b)) => {
            let b = *b;
            // geometry owners are arena identities with the naming left `Unknown`, as
            // this unit's `.smb` import leaves it on main
            let c = counts(&b);
            Ok(Ok((b, ImportTypes::default(), c)))
        }
        Some(Built::Failed(e)) => match Output::unported(&e) {
            Some(o) => Ok(Err(o)),
            None => Ok(Err(Output::err(format!(
                "brep_build_status_{}",
                build_status(&e)
            )))),
        },
        None => {
            let ex = export(i)?;
            let Some((c, graph_at)) = ffi::counts(&ex) else {
                return Ok(Err(build_failure(&ex)));
            };
            let (b, types) = import(&ex, graph_at)?;
            Ok(Ok((b, types, c)))
        }
    }
}

// ---- independent cross-body observation -------------------------------------------------

const INTERSECTION_SAMPLES: [f64; 6] = [0.0, 0.17, 0.37, 0.5, 0.83, 1.0];

fn cross_body_input(i: &Inputs, other: &BrepIn) -> Result<Vec<f64>, HarnessError> {
    if !matches!(i.brep, Some(BrepIn::Face { .. })) || !matches!(other, BrepIn::Face { .. }) {
        return Err(harness(
            "cross-body intersection needs two real face blocks",
        ));
    }
    let mut input = brep_block(i)?;
    input.extend(brep_block_of(Some(other), false, false)?);
    Ok(input)
}

fn independent_face(k: &mut Kernel, input: &BrepIn) -> Result<(Brep, f64), Fail> {
    let BrepIn::Face { surface, dom } = input else {
        return Err(harness("cross-body intersection needs a face").into());
    };
    // Match native `new (ctx) SmBrep()` and the executed kernel fixtures:
    // a bare arena lacks the infinite region and constructor naming/tolerances.
    let mut b = solidcore::topology::brep::p3::new_brep(k, tol::USE_DEFAULT, tol::USE_DEFAULT)?;
    let surface = surface_of(surface)?;
    let mut state = solidcore::topology::brep::p2::ArenaNotifyState::new(&b);
    let mut env = state.env(None);
    let mut marks = Marks::default();
    let this_face = |face| arena_identity(CacheObject::Face(face));
    let mut fenv = solidcore::topology::brep::p8::FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &this_face,
    };
    let domain = Extent2d::from_bounds(dom[0], dom[1], dom[2], dom[3]);
    let status = match solidcore::topology::brep::p8::create_face_from_surface(
        k,
        &mut b,
        Some(surface),
        &domain,
        &mut fenv,
    ) {
        Ok(_) => 0.0,
        Err(e) if e.is_unported() => return Err(e.into()),
        Err(e) => {
            eprintln!("cross-body face construction failed: {e:?}; {e}; domain={domain:?}");
            status_of(&e)
        }
    };
    Ok((b, status))
}

fn put_intersection_curve(k: &Kernel, curve: &Curve, out: &mut Vec<f64>) -> Result<(), Fail> {
    use solidcore::curve::curve::CurveKind;
    let domain = curve.natural_interval();
    out.extend([curve.type_code() as f64, domain.min(), domain.max()]);
    for fraction in INTERSECTION_SAMPLES {
        match curve.evaluate_point(k, domain.evaluate(fraction)) {
            Ok(point) => {
                out.push(0.0);
                out.extend(point.to_array());
            }
            Err(e) if e.is_unported() => return Err(e.into()),
            Err(e) => out.push(status_of(&e)),
        }
    }
    // Preserve dynamic type while observing the inherited native B-spline net.
    // Other concrete kinds still retain every sample and their actual type.
    let spline = match curve.kind() {
        CurveKind::BSpline(c) => Some(c),
        CurveKind::Line(c) => Some(c.base()),
        CurveKind::Ellipse(c) => Some(c.base()),
        CurveKind::Hyperbola(c) => Some(c.base()),
        CurveKind::Parabola(c) => Some(c.base()),
        _ => None,
    };
    out.push(flag(spline.is_some()));
    if let Some(spline) = spline {
        out.extend([spline.dim() as f64, flag(spline.gw_nurb().is_some())]);
        if let Some(net) = spline.gw_nurb() {
            out.extend([net.n() as f64, net.degree() as f64, net.m() as f64]);
            for p in net.cpts() {
                out.extend([p.x, p.y, p.z, p.w]);
            }
            out.extend_from_slice(net.knots());
        }
    }
    Ok(())
}

fn run_rust_intersection(
    i: &Inputs,
    other: &BrepIn,
    tol: f64,
    ang: f64,
    want_points: bool,
) -> Result<Output, HarnessError> {
    cross_body_input(i, other)?; // same input-kind check, no native construction/import
    let run = || -> Result<Output, Fail> {
        let mut k = Kernel::default();
        let first = i
            .brep
            .as_ref()
            .ok_or_else(|| harness("missing first face"))?;
        let (mut b, first_status) = independent_face(&mut k, first)?;
        let (mut second, second_status) = independent_face(&mut k, other)?;
        b.set_validate_only(i.validate_only);
        if i.init_gaps && first_status == 0.0 {
            let vertexuses: Vec<_> = b.vertexuses().iter().map(|(id, _)| id).collect();
            for vu in vertexuses {
                solidcore::topology::vertexuse::vertex_face_gap(&k, &mut b, vu, false)?;
            }
            let edgeuses: Vec<_> = b.edgeuses().iter().map(|(id, _)| id).collect();
            for eu in edgeuses {
                solidcore::topology::edgeuse::p1::max_edge_face_gap(&k, &mut b, eu, false)?;
                solidcore::topology::edgeuse::p1::edge_ccw_edge_gap(&k, &mut b, eu, false)?;
            }
        }
        let ran = first_status == 0.0 && second_status == 0.0;
        let mut out = vec![first_status, second_status, flag(ran)];
        if ran {
            let (curves, points) =
                bp1::brep_intersect_mut(&k, &mut b, &mut second, tol, ang, want_points)?;
            out.extend([
                0.0,
                curves.len() as f64,
                points.as_ref().map_or(-1.0, |p| p.len() as f64),
            ]);
            for curve in &curves {
                put_intersection_curve(&k, curve, &mut out)?;
            }
            if let Some(points) = points {
                for point in points {
                    out.extend(point.to_array());
                }
            }
        }
        Ok(intersection_output(&out)?)
    };
    match run() {
        Ok(out) => Ok(out),
        Err(Fail::Solid(e)) => {
            Output::unported(&e).ok_or_else(|| harness(format!("rust cross-body: {e}")))
        }
        Err(Fail::Harness(e)) => Err(e),
    }
}

/// Checked native-output cursor: counts are bounded by remaining scalars before
/// iteration/copying. No capacity is allocated from an unchecked declared count.
struct IntersectionReader<'a> {
    input: &'a [f64],
    at: usize,
}

impl IntersectionReader<'_> {
    fn take(&mut self, n: usize) -> Result<&[f64], HarnessError> {
        let end = self
            .at
            .checked_add(n)
            .ok_or_else(|| harness("intersection output overflow"))?;
        let values = self
            .input
            .get(self.at..end)
            .ok_or_else(|| harness("short intersection output"))?;
        self.at = end;
        Ok(values)
    }
    fn integer(&mut self) -> Result<i64, HarnessError> {
        let value = self
            .take(1)?
            .first()
            .copied()
            .ok_or_else(|| harness("short intersection output"))?;
        if !value.is_finite() || value.fract() != 0.0 || value.abs() > (1_u64 << 53) as f64 {
            return Err(harness("invalid intersection integer"));
        }
        Ok(value as i64)
    }
    fn count(&mut self, minimum: usize) -> Result<usize, HarnessError> {
        let count =
            usize::try_from(self.integer()?).map_err(|_| harness("negative intersection count"))?;
        if count > self.input.len().saturating_sub(self.at) / minimum {
            return Err(harness("intersection count exceeds available output"));
        }
        Ok(count)
    }
    fn flag(&mut self) -> Result<i64, HarnessError> {
        let value = self.integer()?;
        if !(0..=1).contains(&value) {
            return Err(harness("invalid intersection flag"));
        }
        Ok(value)
    }
}

fn intersection_output(raw: &[f64]) -> Result<Output, HarnessError> {
    let mut rd = IntersectionReader { input: raw, at: 0 };
    let first = rd.integer()?;
    let second = rd.integer()?;
    let ran = rd.flag()?;
    let mut out = Output::new()
        .ints("intersection.build_status", vec![first, second])
        .int("intersection.ran", ran);
    if ran == 1 {
        out = out.int("intersection.status", rd.integer()?);
        let curves = rd.count(10)?;
        let points = rd.integer()?;
        if points < -1 {
            return Err(harness("invalid optional point count"));
        }
        out = out.ints("intersection.counts", vec![curves as i64, points]);
        for i in 0..curves {
            let prefix = format!("intersection.curve.{i}");
            out = out
                .int(&format!("{prefix}.kind"), rd.integer()?)
                .values(&format!("{prefix}.domain"), rd.take(2)?.to_vec());
            for j in 0..INTERSECTION_SAMPLES.len() {
                let status = rd.integer()?;
                out = out.int(&format!("{prefix}.sample.{j}.status"), status);
                if status == 0 {
                    out = out.values(&format!("{prefix}.sample.{j}.xyz"), rd.take(3)?.to_vec());
                }
            }
            let spline = rd.flag()?;
            out = out.int(&format!("{prefix}.is_bspline"), spline);
            if spline == 1 {
                let dim = rd.integer()?;
                let has_nurb = rd.flag()?;
                out = out.ints(&format!("{prefix}.spline"), vec![dim, has_nurb]);
                if has_nurb == 1 {
                    let n = rd.count(4)?;
                    let degree = rd.integer()?;
                    let m = rd.count(1)?;
                    let controls = n
                        .checked_add(1)
                        .and_then(|n| n.checked_mul(4))
                        .ok_or_else(|| harness("intersection net size overflow"))?;
                    let knots = m
                        .checked_add(1)
                        .ok_or_else(|| harness("intersection knot count overflow"))?;
                    out = out
                        .ints(&format!("{prefix}.shape"), vec![n as i64, degree, m as i64])
                        .values(
                            &format!("{prefix}.weighted_controls"),
                            rd.take(controls)?.to_vec(),
                        )
                        .values(&format!("{prefix}.knots"), rd.take(knots)?.to_vec());
                }
            }
        }
        if points >= 0 {
            let count = usize::try_from(points)
                .ok()
                .and_then(|n| n.checked_mul(3))
                .ok_or_else(|| harness("intersection points overflow"))?;
            out = out.values("intersection.points", rd.take(count)?.to_vec());
        }
    }
    if rd.at != raw.len() {
        return Err(harness("trailing intersection output"));
    }
    Ok(out)
}

// ---- generated cases --------------------------------------------------------------------

fn gen_sel(rng: &mut Rng) -> i64 {
    rng.below(1 << 20) as i64
}

fn gen_target(rng: &mut Rng) -> [i64; 2] {
    let ty = match rng.below(14) {
        10 => 11001,
        11 => 0,
        12 => ffi::SURFACE_TYPE,
        13 => ffi::CURVE_TYPE,
        i => TARGET_TYPES.get(i as usize).map_or(0, |t| t.0),
    };
    let idx = if rng.chance(0.1) { -1 } else { gen_sel(rng) };
    [ty, idx]
}

fn gen_op(rng: &mut Rng) -> OpIn {
    match rng.below(27) {
        0 | 1 => {
            let n = rng.below(4) as usize;
            OpIn::Lists {
                which: rng.below(10) as u8,
                attr: if rng.chance(0.5) {
                    -1
                } else {
                    rng.below(50) as i64
                },
                targets: (0..n).map(|_| gen_target(rng)).collect(),
            }
        }
        2 => {
            let [ty, index] = gen_target(rng);
            OpIn::Query { ty, index }
        }
        3 => {
            let value = match rng.below(6) {
                0 => tol::USE_DEFAULT,
                1 => 10f64.powf(rng.uniform(-8.0, 8.0)),
                2 => -rng.uniform(0.0, 100.0),
                3 => 0.0,
                4 => rng.uniform(0.5, 500.0),
                _ => rng.uniform(1e5, 1e7),
            };
            OpIn::SetSize {
                kind: rng.below(2) as u8,
                value,
            }
        }
        4 => OpIn::SetInf {
            set: rng.range_i64(-2, 1 << 10),
            mode: rng.below(3) as u8,
            reset: rng.range_i64(-1, 1 << 10),
            flag: rng.below(3) as u8,
            unsure: if rng.chance(0.3) {
                rng.range_i64(0, 1 << 10)
            } else {
                -1
            },
        },
        5 => OpIn::Degenerate {
            tol: 10f64.powf(rng.uniform(-9.0, 9.0)),
        },
        6 => OpIn::AddDeleted {
            n: rng.below(5) as u8,
        },
        7 => {
            let n = rng.below(10) as usize;
            OpIn::Vote {
                regions: (0..n).map(|_| rng.range_i64(-1, 4)).collect(),
            }
        }
        8 => {
            let ns = rng.below(6) as usize;
            let ng = rng.below(6) as usize;
            OpIn::ObjMap {
                sets: (0..ns)
                    .map(|_| ObjSet {
                        orig: gen_target(rng),
                        copy: gen_target(rng),
                        copy_map: rng.chance(0.5),
                        reverse: rng.chance(0.5),
                    })
                    .collect(),
                gets: (0..ng).map(|_| gen_target(rng)).collect(),
            }
        }
        9 => {
            let n = rng.below(5) as usize;
            let m = rng.below(6) as usize;
            OpIn::AddAttrs {
                target: gen_target(rng),
                ids: (0..n).map(|_| rng.below(5) as i64).collect(),
                indexed: (0..m).map(|_| gen_sel(rng)).collect(),
            }
        }
        10 => OpIn::ReflectEu { edge: gen_sel(rng) },
        11 => OpIn::ReflectShell { face: gen_sel(rng) },
        12 => OpIn::RmLoopVu { vu: gen_sel(rng) },
        13 => OpIn::MarkLoops {
            loop_: gen_sel(rng),
            level: rng.below(4),
        },
        14 => OpIn::SwapFaceUv {
            face: gen_sel(rng),
            mark: *rng.choose(&[1u8, 2, 4, 8, 16]).unwrap_or(&1),
            premark: rng.chance(0.7),
        },
        15 => OpIn::CheckUv { eu: gen_sel(rng) },
        16 => OpIn::LockBox,
        17 => {
            let curve = gen_curve(rng, 3, 1.0);
            let p = rng.point3(-3.0, 3.0);
            let dir = match rng.below(4) {
                0 => [0.0, 0.0, 1.0],
                1 => [1.0, 0.0, 0.0],
                _ => rng.point3(-1.0, 1.0),
            };
            let tol = match rng.below(3) {
                0 => 0.0,
                1 => 1e-6,
                _ => 10f64.powf(rng.uniform(-9.0, 0.0)),
            };
            OpIn::FindSide { curve, p, dir, tol }
        }
        18 => OpIn::FindSector {
            face: gen_sel(rng),
            guess: rng
                .chance(0.5)
                .then(|| [rng.uniform(-1.0, 2.0), rng.uniform(-1.0, 2.0)]),
            p: rng.point3(-5.0, 5.0),
            dir: rng.point3(-1.0, 1.0),
        },
        19 => OpIn::UpdateIvls {
            vertex: gen_sel(rng),
        },
        20 => OpIn::Redistribute {
            edge: gen_sel(rng),
            has_uv: rng.chance(0.5),
            // SM_SP_U (0) ..= SM_SP_VMAX (8)
            seam: rng.below(9) as u8,
            dom: [0.0, 0.0, 1.0, 1.0],
        },
        21 => OpIn::SplitUv {
            eu_orig: gen_sel(rng),
            eu_new: gen_sel(rng),
            p: rng.point3(-2.0, 2.0),
            t: rng.uniform(-0.2, 1.2),
            scaled_zero: 10f64.powf(rng.uniform(-9.0, -1.0)),
            orient: rng.below(3) as u8,
            for_tess: rng.chance(0.5),
        },
        22 => OpIn::BrepIntersect {
            tol: 10f64.powf(rng.uniform(-6.0, -2.0)),
            ang: rng.uniform(5.0, 30.0),
            want_points: rng.chance(0.5),
        },
        23 => {
            let n = rng.below(4) as usize;
            OpIn::CornerBlend {
                eus: (0..n).map(|_| gen_sel(rng)).collect(),
                tangent: rng
                    .chance(0.5)
                    .then(|| (0..n).map(|_| rng.chance(0.7)).collect()),
                tol: 10f64.powf(rng.uniform(-6.0, -2.0)),
                ang: rng.uniform(0.0, 0.1),
            }
        }
        24 => OpIn::EdgeEdgeBlend {
            flag: rng.below(4) as u8,
            eu1: if rng.chance(0.05) { -1 } else { gen_sel(rng) },
            eu2: if rng.chance(0.05) { -1 } else { gen_sel(rng) },
            dir: rng.below(3) as u8,
        },
        25 => OpIn::ParallelProj {
            curve: gen_curve(rng, 3, 1.0),
            vec: match rng.below(3) {
                0 => [0.0, 0.0, -1.0],
                1 => [0.0, 0.0, 1.0],
                _ => rng.point3(-1.0, 1.0),
            },
            dist: rng.chance(0.3).then(|| rng.uniform(0.5, 20.0)),
            tol: rng.chance(0.7).then(|| 10f64.powf(rng.uniform(-6.0, -2.0))),
            ang: rng.chance(0.5).then(|| rng.uniform(0.05, 0.5)),
            want_3d: rng.chance(0.8),
            want_uv: rng.chance(0.8),
            want_faces: rng.chance(0.5),
        },
        _ => OpIn::Heal {
            // SM_HO_NONE (0) ..= SM_HO_ALL (26)
            healer_op: rng.below(27) as u8,
            make_uv: rng.chance(0.5),
        },
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.brep_p1"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        run_cpp(&case.parse::<Inputs>()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        run_rust(&case.parse::<Inputs>()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let brep = if rng.chance(0.2) {
            BrepIn::File {
                file: (*rng.choose(&FILES).unwrap_or(&"Mouse.smb")).to_string(),
                rebuild: rng.chance(0.25),
            }
        } else {
            gen_brep(rng)
        };
        // an op needing faces or loops compares nothing on a wire B-rep: draw again
        let has_faces = matches!(brep, BrepIn::File { .. } | BrepIn::Face { .. });
        let mut op = gen_op(rng);
        for _ in 0..8 {
            let needs_faces = matches!(
                op,
                OpIn::ReflectShell { .. }
                    | OpIn::MarkLoops { .. }
                    | OpIn::SwapFaceUv { .. }
                    | OpIn::FindSector { .. }
            );
            if has_faces || !needs_faces {
                break;
            }
            op = gen_op(rng);
        }
        let i = Inputs {
            brep: Some(brep),
            validate_only: rng.chance(0.15),
            init_gaps: rng.chance(0.3),
            op,
        };
        serde_json::to_value(i).ok()
    }
}

#[cfg(test)]
mod cross_body_tests {
    use super::*;
    use crate::compare::compare;

    fn transverse() -> Inputs {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/topology.brep_p1/cross_body.json"
        ))
        .unwrap();
        let case = fixture["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["name"] == "cross_body_transverse")
            .unwrap();
        serde_json::from_value(case["inputs"].clone()).unwrap()
    }

    #[test]
    fn cross_body_actual_geometry_and_targeted_sensitivity() {
        let i = transverse();
        let cpp = run_cpp(&i).unwrap();
        let rust = run_rust(&i).unwrap();
        let diff = compare(&cpp, &rust, &[]);
        assert!(diff.pass(), "{diff:?}");
        assert_eq!(cpp.ints["intersection.counts"], vec![1, 0]);
        assert_eq!(cpp.ints["intersection.curve.0.kind"], vec![14033]);
        assert!(cpp.values["intersection.curve.0.weighted_controls"].len() >= 12);
        for key in [
            "intersection.curve.0.domain",
            "intersection.curve.0.sample.1.xyz",
            "intersection.curve.0.weighted_controls",
            "intersection.curve.0.knots",
        ] {
            let mut changed = rust.clone();
            changed.values.get_mut(key).unwrap()[0] += 0.125;
            assert!(
                !compare(&cpp, &changed, &[]).pass(),
                "missed perturbation: {key}"
            );
        }
        let mut changed = rust.clone();
        changed.ints.get_mut("intersection.curve.0.kind").unwrap()[0] += 1;
        assert!(
            !compare(&cpp, &changed, &[]).pass(),
            "missed dynamic curve kind"
        );
        let mut changed = rust;
        changed.ints.get_mut("intersection.counts").unwrap()[1] = -1;
        changed.values.remove("intersection.points");
        assert!(
            !compare(&cpp, &changed, &[]).pass(),
            "missed optional point presence"
        );
    }

    #[test]
    fn cross_body_decoder_bounds_and_optional_points() {
        let enabled = intersection_output(&[0., 0., 1., 0., 0., 0.]).unwrap();
        let disabled = intersection_output(&[0., 0., 1., 0., 0., -1.]).unwrap();
        assert!(enabled.values.contains_key("intersection.points"));
        assert!(!disabled.values.contains_key("intersection.points"));
        assert!(!compare(&enabled, &disabled, &[]).pass());
        for raw in [
            vec![],
            vec![0., 0., 1., 0., 1e15, -1.],
            vec![0., 0., 1., 0., 0., 1e15],
            vec![0., 0., 1., 0., 0., -2.],
            vec![0., 0., 0., 42.],
        ] {
            assert!(intersection_output(&raw).is_err());
        }
        let failure = intersection_output(&[1007., 0., 0.]).unwrap();
        assert_eq!(failure.ints["intersection.build_status"], vec![1007, 0]);
        assert!(!failure.ints.contains_key("intersection.status"));
        // Unknown non-B-spline types are observed, not skipped or flattened.
        let mut raw = vec![0., 0., 1., 0., 1., -1., 19999., 0., 1.];
        for _ in INTERSECTION_SAMPLES {
            raw.extend([0., 1., 2., 3.]);
        }
        raw.push(0.);
        let unknown = intersection_output(&raw).unwrap();
        assert_eq!(unknown.ints["intersection.curve.0.kind"], vec![19999]);
        assert_eq!(
            unknown.values["intersection.curve.0.sample.5.xyz"],
            vec![1., 2., 3.]
        );
        for end in 0..raw.len() {
            assert!(intersection_output(&raw[..end]).is_err());
        }
    }
}
