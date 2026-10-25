//! Difftest runner for unit `heal.heal_data_p1`: the `SmHealData` entry points and
//! the stage 2 / stage 3 face, sheet, trim curve and infinite region steps, and
//! `SmApiHealBrep`.
//!
//! A case is either a heal script of the `heal.heal_data_p2` runner (the healer built
//! on a real B-rep, steps `23..=31` of
//! [`solidcore_ffi::units::heal_heal_data_p2::check_heal`] driving this unit's
//! functions after the p2 cache steps, followed by the whole healer state), or an
//! `SmApiHealBrep` call (`{"api": {"box_size": [dx, dy, dz]}}`, or without a box
//! for a null B-rep).
//!
//! Generated scripts run the p2 cache sequence on boxes (1e-4..1e4), sphere faces,
//! upstream test files and the generated B-reps of `topology.vertexuse`, with random
//! `m_eDoneHealOp` values (before and after `Fix_TolSizes` / `Fix_SplitEdgesAtSeam`,
//! which gate stage 2 and stage 3), random target subsets, re-runs on a tail of the
//! targets, not-yet moves, and one of `ReSet`, `Fix_InfiniteRegion` or `HealBrep`
//! last.
//!
//! Deferred: `Cache_FaceProps_Stage2` on closed surfaces (`SmFace::HasSeamProblem`,
//! `topology.face_p4`), `Cache_FaceProps_Stage3` past its op gate
//! (`SmFace::ClassifyLoops`, `query.face_p4`), `Make_UVTrimCurves` making curves
//! (`topology.face_p3`), `SmApiHealBrep` on a box (the Rust side cannot build it:
//! `SmApiCreateBox`, `primitive.api_primitives`), and whatever the p2 steps before them
//! defer (`Fix_InfiniteRegion` and `HealBrep` included).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::heal::heal_data::p1::api_heal_brep;
use solidcore::kernel::Kernel;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::SolidError;
use solidcore_ffi::units::heal_heal_data_p1 as ffi;

use super::heal_heal_data_p2::{
    gen_brep_in, gen_targets, log_uniform, run_cpp as p2_cpp, run_rust as p2_rust, BIn,
    Inputs as Script, Prim, Step,
};
use super::topology_vertexuse::{domain, gen_surface, BrepIn};
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 320;

/// `SM_ERR_INVALID_INPUT`.
const SM_ERR_INVALID_INPUT: f64 = 1007.0;
/// `SM_SUCCESS`.
const SM_SUCCESS: f64 = 1000.0;

/// The difftest runner.
pub struct Runner;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Api {
    /// The `SmApiCreateBox` sides; `None` heals a null B-rep.
    #[serde(default)]
    box_size: Option<[f64; 3]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum Inputs {
    Api { api: Api },
    Script(Box<Script>),
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// Largest magnitude of a script value the C++ side is given (the `±f64::MAX` markers
/// aside). HANG-29: a B-rep coordinate whose square overflows (from about `1.34e154`)
/// makes the healer's face work (`SmCurve::GlobalCoincidenceChecker` through
/// `DropPoint`) split forever; weights stay within `1e-3..=1e3`, so this bound keeps
/// every Euclidean point under that regime.
const MAX_MAGNITUDE: f64 = 1.0e150;

/// Whether `v` holds a number above [`MAX_MAGNITUDE`] (other than `±f64::MAX`).
fn has_huge(v: &Value) -> bool {
    match v {
        Value::Number(n) => n.as_f64().is_some_and(|x| {
            let a = x.abs();
            a > MAX_MAGNITUDE && a < f64::MAX
        }),
        Value::Array(a) => a.iter().any(has_huge),
        Value::Object(o) => o.values().any(has_huge),
        _ => false,
    }
}

fn run_cpp(i: &Inputs) -> Result<Output, HarnessError> {
    match i {
        Inputs::Script(s) => {
            let v = serde_json::to_value(s).map_err(|e| harness(e.to_string()))?;
            if has_huge(&v) {
                return Err(harness("script value above 1e150 (HANG-29)"));
            }
            p2_cpp(s)
        }
        Inputs::Api { api } => {
            let r = match api.box_size {
                None => ffi::api_heal_null(),
                Some(d) => ffi::api_heal_box(d),
            };
            match r {
                Ok(v) => Ok(Output::new().values("out", v)),
                Err(solidcore_ffi::FfiError::Status(s)) => Ok(Output::err(format!("status_{s}"))),
                Err(e) => Err(harness(format!("c++: {e}"))),
            }
        }
    }
}

fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    match i {
        Inputs::Script(s) => {
            // as the C++ side: the Rust side builds its B-rep through the same C++
            // export, which hangs under HANG-29
            let v = serde_json::to_value(s).map_err(|e| harness(e.to_string()))?;
            if has_huge(&v) {
                return Err(harness("script value above 1e150 (HANG-29)"));
            }
            p2_rust(s)
        }
        Inputs::Api { api } => {
            let mut k = Kernel::default();
            // The C++ side heals a box made by `SmApiCreateBox(origin, dx, dy, dz)`;
            // the output is `create_status heal_status`.
            let mut create_status = 0.0;
            let r = match api.box_size {
                None => api_heal_brep(&mut k, None),
                Some([dx, dy, dz]) => {
                    match create_box(&mut k, Point3::new(0.0, 0.0, 0.0), dx, dy, dz) {
                        Ok(mut b) => {
                            create_status = SM_SUCCESS;
                            api_heal_brep(&mut k, Some(&mut b))
                        }
                        Err(SolidError::InvalidInput { .. }) => {
                            return Ok(Output::new().values("out", vec![SM_ERR_INVALID_INPUT, 0.0]));
                        }
                        Err(e) => {
                            return Ok(Output::unported(&e)
                                .unwrap_or_else(|| Output::err(e.to_string())));
                        }
                    }
                }
            };
            Ok(match r {
                Ok(()) => Output::new().values("out", vec![create_status, SM_SUCCESS]),
                Err(SolidError::InvalidInput { .. }) => {
                    Output::new().values("out", vec![create_status, SM_ERR_INVALID_INPUT])
                }
                Err(e) => Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string())),
            })
        }
    }
}

// ---- generation -------------------------------------------------------------------------

/// A box, a sphere face, an upstream file, a generated B-spline face or another
/// generated B-rep, and whether the C++ healer may run on it to the end (`HealBrep`,
/// `Fix_InfiniteRegion`). Generated B-spline faces dominate: stage 2 on a plane face
/// stops at the plane's face tolerance (`SmFace::GetTolerance` in the plane's box,
/// `topology.face_p2`) and on a closed surface at `SmFace::HasSeamProblem`.
fn gen_brep(rng: &mut Rng) -> (BIn, bool) {
    match rng.below(20) {
        0..=2 => {
            let lo = if rng.chance(0.15) { 1e-4 } else { 1e-2 };
            let hi = if rng.chance(0.15) { 1e4 } else { 1e2 };
            (
                BIn::Prim(Prim::Box {
                    size: [
                        log_uniform(rng, lo, hi),
                        log_uniform(rng, lo, hi),
                        log_uniform(rng, lo, hi),
                    ],
                }),
                true,
            )
        }
        3 => (
            BIn::Prim(Prim::Sphere {
                radius: log_uniform(rng, 1e-2, 1e2),
                swap: rng.chance(0.5),
            }),
            false,
        ),
        4..=6 => {
            const FILES: [&str; 8] = [
                "surf_extrude.smb",
                "surf_rev.smb",
                "vase_rev.smb",
                "ellips_ts2.smb",
                "plane_square.smb",
                "disk.smb",
                "cone.smb",
                "Endcap.smb",
            ];
            let f = *rng.choose(&FILES).unwrap_or(&"surf_extrude.smb");
            (
                BIn::Std(BrepIn::File {
                    file: f.to_string(),
                    rebuild: rng.chance(0.25),
                }),
                false,
            )
        }
        7..=17 => {
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
            (BIn::Std(BrepIn::Face { surface: s, dom }), false)
        }
        _ => (gen_brep_in(rng), false),
    }
}

/// A done-op value around the stage gates: below `Fix_TolSizes` (5), between it and
/// `Fix_SplitEdgesAtSeam` (19), or at/after it.
fn gen_done_op(rng: &mut Rng) -> u8 {
    match rng.below(6) {
        0 => rng.below(5) as u8,
        1 => 4,
        2 => 5,
        3 => 5 + rng.below(14) as u8,
        4 => 18,
        _ => 19 + rng.below(8) as u8,
    }
}

fn gen_script(rng: &mut Rng) -> Script {
    let (brep, full_ok) = gen_brep(rng);
    // the edge and vertex caches often stop at their gap evaluations; stage 2 needs
    // only the face gap step
    let mut steps = Vec::new();
    if rng.chance(0.2) {
        steps.extend([Step::CacheEdgeProps, Step::CacheVertexProps]);
    }
    steps.push(Step::CacheFacePropsGaps);
    if rng.chance(0.2) {
        steps.push(Step::CacheCoinVertices);
    }
    steps.push(Step::SetDoneHealOp {
        op: gen_done_op(rng),
    });
    // occasionally skip the gap step (stage 2 then meets props without a face)
    if rng.chance(0.04) {
        steps.retain(|s| !matches!(s, Step::CacheFacePropsGaps));
    }
    steps.push(Step::CacheFacePropsStage2);
    if rng.chance(0.6) {
        steps.push(Step::FaceSnapshot);
    }
    if rng.chance(0.2) {
        steps.push(Step::MoveToNotYet { sel: rng.below(64) });
    }
    if rng.chance(0.7) {
        steps.push(Step::FixBadSheets);
    }
    if rng.chance(0.3) {
        steps.push(Step::RemoveFaceProps { sel: rng.below(64) });
    }
    if rng.chance(0.5) {
        steps.push(Step::SetDoneHealOp {
            op: gen_done_op(rng),
        });
    }
    steps.push(Step::CacheFacePropsStage3);
    steps.push(Step::MakeUvTrimCurves {
        make: rng.chance(0.15),
    });
    // incremental re-run on a tail of the targets
    if rng.chance(0.25) {
        steps.push(Step::LoadTargets {
            targets: gen_targets(rng),
        });
        steps.push(Step::SetOldCounts {
            counts: [rng.below(4), rng.below(8), rng.below(4)],
        });
        steps.extend([
            Step::CacheFacePropsGaps,
            Step::CacheFacePropsStage2,
            Step::FixBadSheets,
        ]);
        if rng.chance(0.5) {
            steps.push(Step::CacheFacePropsStage3);
        }
    }
    if rng.chance(0.3) {
        steps.push(Step::FaceSnapshot);
    }
    match rng.below(12) {
        0 | 1 => steps.push(Step::Reset {
            new_obj_arrays: rng.chance(0.5),
        }),
        2 => steps.push(Step::HealBrep {
            mode: 0,
            op: rng.below(27) as u8,
            make: rng.chance(0.5),
            targets: None,
        }),
        3 if full_ok => steps.push(Step::FixInfiniteRegion),
        4 if full_ok => steps.push(Step::HealBrep {
            mode: 1 + rng.below(2) as u8,
            op: rng.below(27) as u8,
            make: false,
            targets: Some(gen_targets(rng)),
        }),
        _ => {}
    }
    Script::Heal {
        brep: Box::new(brep),
        validate_only: rng.chance(0.05),
        init_gaps: rng.chance(0.5),
        targets: gen_targets(rng),
        voxels: [12, 12, 12],
        steps,
    }
}

fn gen_api(rng: &mut Rng) -> Inputs {
    let box_size = if rng.chance(0.4) {
        None
    } else {
        Some([
            log_uniform(rng, 1e-2, 1e2),
            log_uniform(rng, 1e-2, 1e2),
            log_uniform(rng, 1e-2, 1e2),
        ])
    };
    Inputs::Api {
        api: Api { box_size },
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "heal.heal_data_p1"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let c = if rng.chance(0.04) {
            gen_api(rng)
        } else {
            Inputs::Script(Box::new(gen_script(rng)))
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

    #[test]
    fn inputs_round_trip() {
        let mut rng = Rng::new(7);
        for _ in 0..50 {
            let v = Runner.generate(&mut rng).unwrap();
            let i: Inputs = serde_json::from_value(v.clone()).unwrap();
            assert_eq!(serde_json::to_value(&i).unwrap(), v);
        }
    }
}
