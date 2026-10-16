//! Differential coverage of `heal.heal_data_p3` on real native B-reps.
//! Shared scripts compare all props, flags, problem-list order and counters.
//! `HealTgts` is terminal because it can delete fixture topology; that operation
//! compares status, stop/done step, recursion count and resulting target counts.
//! Mutating branches defer at the exact missing topology/face/classification call.

use super::heal_heal_data_p2::{self as shared, BIn, Inputs, Prim, Sel, Step, Targets};
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde_json::Value;

/// Differential runner for the healer's third unit.
pub struct Runner;

fn targets(all: bool) -> Targets {
    let s = if all { Sel::All } else { Sel::None };
    Targets {
        v: s.clone(),
        e: s.clone(),
        f: s,
    }
}

fn script(rng: &mut Rng) -> Inputs {
    let mode = rng.below(10);
    let brep = if mode < 3 {
        BIn::Prim(Prim::Box {
            size: [
                shared::log_uniform(rng, 1e-4, 1e4),
                shared::log_uniform(rng, 1e-4, 1e4),
                shared::log_uniform(rng, 1e-4, 1e4),
            ],
        })
    } else {
        shared::gen_brep_in(rng)
    };
    let mut t = targets(mode != 0);
    let mut steps = Vec::new();
    match mode {
        0..=2 => steps.push(Step::HealTargets {
            op: rng.below(27) as u8,
            make: rng.chance(0.5),
        }),
        3 => {
            t = targets(false);
            steps.push(Step::SetDoneHealOp {
                op: rng.below(5) as u8,
            });
            steps.push(Step::UpdateTargets {
                op: rng.below(27) as u8,
                remove: vec![],
                add: vec![[9, 0], [7, 0], [3, 0], [10, 0], [1, 0]],
            });
        }
        4 => {
            steps.extend([Step::CacheFacePropsGaps, Step::CacheFacePropsStage2]);
            steps.push(Step::FixMoveSeam);
            steps.push(Step::FixSplitEdgesAtSeam);
            steps.push(Step::FixSplitFaceAtSeams);
        }
        5 => {
            steps.push(Step::CacheEdgeProps);
            steps.push(Step::FixDegenEdges);
            steps.push(Step::CacheVertexProps);
            steps.push(Step::CacheCoinVertices);
            steps.push(Step::FixCoinVertices);
        }
        6 => {
            steps.push(Step::CacheDegenFaces);
            steps.push(Step::FixDegenFaces);
        }
        7 => {
            steps.push(Step::SetDoneHealOp {
                op: rng.below(27) as u8,
            });
            steps.push(Step::UpdateTargets {
                op: rng.below(27) as u8,
                remove: vec![[9, 0], [7, 0], [3, 0]],
                add: vec![],
            });
        }
        8 => steps.push(Step::UpdateTracked {
            active: rng.chance(0.5),
            op: rng.below(27) as u8,
        }),
        _ => steps.extend([
            Step::FixCoinVertices,
            Step::FixDegenFaces,
            Step::FixDegenEdges,
            Step::FixMoveSeam,
            Step::FixSplitEdgesAtSeam,
            Step::FixSplitFaceAtSeams,
        ]),
    }
    Inputs::Heal {
        brep: Box::new(brep),
        validate_only: false,
        init_gaps: rng.chance(0.5),
        targets: t,
        voxels: [12, 12, 12],
        steps,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "heal.heal_data_p3"
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(script(rng)).ok()
    }
    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        default_cases(
            self,
            &CaseContext {
                seeds: ctx.seeds.max(360),
                ..ctx.clone()
            },
        )
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        shared::run_cpp(&case.parse()?)
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        shared::run_rust(&case.parse()?)
    }
}
