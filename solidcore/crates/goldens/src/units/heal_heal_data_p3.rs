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
                [(2e-5, 3.388_131_789_017_201_4e-21), (2e-5, 3.388_131_789_017_201_4e-21)],
            ),
            (
                "degen_props_sliver_w2e-05_s1_a0",
                [(2e-5, 3.388_131_789_017_201_4e-21), (2e-5, 3.388_131_789_017_201_4e-21)],
            ),
            (
                "degen_props_sliver_w2e-05_s1_a0.7",
                [
                    (2e-5, 2.395_770_963_567_772e-21),
                    (9.111_111_111_127_546e-6, 1.110_223_024_625_156_5e-16),
                ],
            ),
            (
                "degen_props_sliver_w2e-05_s10_a0.7",
                [
                    (9.999_999_999_999_999e-6, 2.395_770_963_567_772e-21),
                    (8.888_888_888_645_883e-6, 1.986_027_322_597_818_5e-15),
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

    /// WR-57 (#17856), the moved rows `degen_props_*` with a 1e-5 sliver: the sliver's far
    /// vertex is its bilinear surface's corner, so its vertex-face gap is 0. Main recorded
    /// 7.4e-6 or 1e-5 there (its drop answered the parameters of the opposite corner);
    /// every cached vertex-face gap is now certified ([`shared::certificates::vertex_face_gaps`]) in an enclosure of
    /// the vertex's distance to its surface, and each below 1e-11 (main's values lie
    /// above it). The problem flags and histogram counts that moved follow
    /// from the gaps.
    #[test]
    fn wr57_sliver_vertex_face_gaps_are_enclosed() {
        for name in [
            "degen_props_curved_sliver",
            "degen_props_sliver_w1e-05_s10_a0",
            "degen_props_sliver_w1e-05_s10_a0.7",
            "degen_props_sliver_w1e-05_s1_a0",
            "degen_props_sliver_w1e-05_s1_a0.7",
        ] {
            let case = crate::case::regression_case(&Runner, name);
            let script: Inputs = case.parse().expect("a heal script");
            // (the state after the property caching, the script's second step: its next
            // step ends the script with a status, before any dump)
            let gaps = shared::certificates::vertex_face_gaps(name, &script, 2);
            assert!(!gaps.is_empty(), "{name}: no cached vertex-face gap");
            assert!(gaps.iter().all(|g| g.gap < 1e-11 && g.enclosure.hi < 1e-11), "{name}: {gaps:?}");
        }
    }

    /// Regression (#18923): splitting a cone face at its seam moves edge uses whose trims run
    /// past the source surface's natural domain (to `u = 1.125` on `[0, 1]`) onto the new
    /// face, whose analytic copy keeps the source's parameterisation on that domain only (it
    /// clamps there; 0.765 off the edge). The split drops such trims, so each trim the new
    /// face's edges 3 and 4 hold when the healer writes its B-rep is one made again on the
    /// copy, certified within the edge's tolerance over the whole edge.
    #[test]
    fn seam_cone_split_drops_trims_off_the_domain() {
        let k = solidcore::kernel::Kernel::default();
        let case = crate::case::regression_case(&Runner, "split_face_seam_cone_r1_1_h1_s90_q0");
        crate::certify::watch_written();
        Runner.run_rust(&case).expect("the seam cone case runs");
        let written = crate::certify::take_written();
        let b = written.first().expect("the healed B-rep");
        let edges = solidcore::topology::brep::p1::edges(b, None).expect("edges");
        let mut checked = 0;
        for &e in edges.iter().filter(|e| [3, 4].contains(&e.index())) {
            let tol = b.edge(e).expect("edge").tolerance;
            // Without the fix the kept trim is 0.765 off its edge: a tolerance below that is
            // what makes this test fail then, so a later raise of it cannot hide the trim.
            assert!(
                tol < 0.765,
                "edge {}: tolerance {tol:e} would cover the kept trim's gap",
                e.index()
            );
            for (eu, bound) in crate::certify::trim_tolerance::trim_bounds_any_surface(&k, b, e) {
                let bound = bound
                    .unwrap_or_else(|| panic!("edge {} use {}: not enclosed", e.index(), eu.index()));
                assert!(
                    bound <= tol,
                    "edge {} use {}: B {bound:e} > tolerance {tol:e}",
                    e.index(),
                    eu.index()
                );
                checked += 1;
            }
        }
        assert!(checked > 0, "edges 3 and 4 of the healed B-rep hold no trim");
        eprintln!("seam_cone_split_drops_trims_off_the_domain: {checked} trims certified");
    }
}
