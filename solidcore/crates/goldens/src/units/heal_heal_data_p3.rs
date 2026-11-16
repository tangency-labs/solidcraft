//! Runner for `heal.heal_data_p3` (upstream `SmHealData`'s repair and target steps, ported
//! into `solidcore::heal::heal_data`) on real B-reps, checked against the unit's golden.
//! Shared scripts record all props, flags, problem-list order and counters.
//! `HealTgts` is terminal because it can delete fixture topology; that operation
//! records status, stop/done step, recursion count and resulting target counts.
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
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        shared::run_rust(&case.parse()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WR-56 (#17467), the moved rows `degen_props_sliver_w2e-05_*` (and `..._s10_a0.7`,
    /// whose raw output moved under an unchanged digest): a 1 x 2e-5 sliver face whose
    /// edges are its bilinear surface's boundary isos, so every edge-face gap is 0 by
    /// construction. Before WR-56 two edges recorded gaps of the sliver's width (2e-5, or
    /// about half of it), and the healer flagged them; each edge-face gap is now certified
    /// ([`shared::certificates::moved_row`]) inside an enclosure below 1e-11, a millionth
    /// of the width. The problem flags, histogram counts and report text that moved
    /// follow from the gaps.
    #[test]
    fn wr56_sliver_edge_face_gaps_are_enclosed() {
        let rows: [(&str, [(f64, f64); 2]); 4] = [
            (
                "degen_props_sliver_w2e-05_s10_a0",
                [(2e-5, 3.388_131_789_017_201_4e-21), (2e-5, 2.964_615_315_390_051e-21)],
            ),
            (
                "degen_props_sliver_w2e-05_s1_a0",
                [(2e-5, 4.235_164_736_271_502e-22), (2e-5, 2.541_098_841_762_901e-21)],
            ),
            (
                "degen_props_sliver_w2e-05_s1_a0.7",
                [
                    (2e-5, 2.395_770_963_567_772e-21),
                    (9.111_111_111_127_546e-6, 1.570_092_458_683_775_2e-16),
                ],
            ),
            (
                "degen_props_sliver_w2e-05_s10_a0.7",
                [
                    (9.999_999_999_999_999e-6, 2.395_770_963_567_772e-21),
                    (8.888_888_888_645_883e-6, 1.776_356_839_400_250_5e-15),
                ],
            ),
        ];
        for (name, moved) in rows {
            let gaps = shared::certificates::moved_row(&Runner, name, &moved);
            for g in &gaps {
                assert!(g.enclosure.hi < 1e-11, "{name}: {g:?}");
            }
        }
    }
}
