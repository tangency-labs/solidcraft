//! A panic fuzz of `flat::decode` (#2490), ported from step-brep's `tests/panic_fuzz.rs` with its
//! cases when step-brep's import was deleted (RFC 0003, 1.6): a body's flat array with values
//! replaced by hostile ones (counts, kinds and coordinates alike) or cut short. A result of `Err`
//! is fine; a panic fails the test with the reproducing seed.
//!
//! The harness is solidcore's (`crates/solidcore/tests/support/panic_fuzz.rs`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "../../solidcore/tests/support/panic_fuzz.rs"]
mod panic_fuzz;

use panic_fuzz::{assert_no_findings, fuzz, Family, Rng, Settings};
use solidcore::CancelToken;
use step_export::Limits;

/// The files whose bodies are encoded: the STEP fixtures step-brep's fuzz started from.
fn bases() -> &'static [&'static [u8]] {
    &[
        include_bytes!("../../solidcore-step/tests/fixtures/discord-2026-10-30-fillet.step"),
        include_bytes!("../src/units/topology_brep_constructor_tetra.stp"),
    ]
}

/// `flat::decode` on a body's array with values replaced by hostile ones (counts,
/// kinds and coordinates alike) or the array cut short.
#[derive(Clone, Debug)]
struct FlatCase {
    base: usize,
    edits: Vec<(f64, f64)>,
    truncate: Option<f64>,
}

struct FlatDecode;

impl Family for FlatDecode {
    type Input = FlatCase;

    fn name(&self) -> &'static str {
        "step-flat"
    }

    fn generate(&self, r: &mut Rng) -> FlatCase {
        FlatCase {
            base: r.below(bases().len()),
            edits: (0..=r.below(4))
                .map(|_| {
                    let v = if r.chance(0.5) {
                        r.pick(panic_fuzz::HOSTILE)
                    } else {
                        r.pick(&[-1.0, 0.0, 1.0, 2.0, 3.0, 7.0, 1e9, 4294967296.0, 0.5, 1e19])
                    };
                    (r.unit(), v)
                })
                .collect(),
            truncate: r.chance(0.3).then(|| r.unit()),
        }
    }

    fn run(case: FlatCase, _cancel: &CancelToken) {
        let Ok(model) = step_export::import(bases()[case.base], &Limits::default()) else {
            return;
        };
        for body in &model.bodies {
            let mut flat = goldens::flat::encode(body);
            if flat.is_empty() {
                continue;
            }
            for &(at, v) in &case.edits {
                let i = ((at * flat.len() as f64) as usize).min(flat.len() - 1);
                flat[i] = v;
            }
            if let Some(at) = case.truncate {
                flat.truncate((at * flat.len() as f64) as usize);
            }
            let _ = goldens::flat::decode(&flat);
        }
    }
}

#[test]
fn panic_fuzz_step_flat_decode() {
    let findings = fuzz(&FlatDecode, &Settings::for_family("step-flat", 200));
    assert_no_findings("step-flat", &findings);
}
