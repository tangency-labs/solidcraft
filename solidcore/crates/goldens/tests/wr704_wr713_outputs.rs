//! WR-704 / WR-713 (`cone_iso_circle_at_axial_height` and
//! `cone_unbounded_parameterized_as_nurb` retired, part of #14246): writes the full outputs of
//! the cases of a moved-row inventory, for `wr704_wr713_cone_moves.rs`, which needs the
//! retired code's outputs in full where the goldens keep only block sums.
//!
//! It uses nothing the retirement added, so it runs unchanged on the parent commit. Run there
//! (copy this file in) and here:
//! `WR713_INVENTORY=<jsonl> WR713_OUTPUTS=<out.jsonl> cargo test --profile goldens -p goldens --test wr704_wr713_outputs -- --ignored`
//! Each line written is `{"unit", "case", "values": {key: [bits]}}`, every float as the hex of
//! its bits (exact). The proof re-encodes these outputs and requires the golden lines of the
//! inventory, so outputs that are not the ones the goldens were blessed from are refused.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::print_stdout
)] // test

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use goldens::case::repo_root;
use goldens::{registry, CaseContext};
use serde_json::Value;

/// The goldens' default number of generated cases per unit (the seeds they were blessed with).
const SEEDS: usize = 16;

#[test]
#[ignore = "writes outputs for wr704_wr713_cone_moves; needs WR713_INVENTORY and WR713_OUTPUTS"]
fn write_outputs() {
    let inventory = std::env::var("WR713_INVENTORY").expect("WR713_INVENTORY=<jsonl>");
    let path = std::env::var("WR713_OUTPUTS").expect("WR713_OUTPUTS=<jsonl>");
    let mut wanted: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for line in std::fs::read_to_string(inventory).unwrap().lines() {
        let r: Value = serde_json::from_str(line).unwrap();
        wanted
            .entry(r["unit"].as_str().unwrap().to_owned())
            .or_default()
            .insert(r["case"].as_str().unwrap().to_owned());
    }
    let ctx = CaseContext {
        root: repo_root(),
        seeds: SEEDS,
    };
    let mut text = String::new();
    let mut written = 0usize;
    for unit in registry() {
        let Some(cases) = wanted.get(unit.id()) else {
            continue;
        };
        for case in unit.cases(&ctx).unwrap() {
            if !cases.contains(&case.name) {
                continue;
            }
            let out = unit.run_rust(&case).unwrap();
            let values: serde_json::Map<String, Value> = out
                .values
                .iter()
                .map(|(k, v)| {
                    let bits: Vec<Value> = v
                        .iter()
                        .map(|x| Value::String(format!("{:016x}", x.to_bits())))
                        .collect();
                    (k.clone(), Value::Array(bits))
                })
                .collect();
            let line = serde_json::json!({"unit": unit.id(), "case": case.name, "values": values});
            writeln!(text, "{line}").unwrap();
            written += 1;
        }
    }
    let expected: usize = wanted.values().map(BTreeSet::len).sum();
    assert_eq!(written, expected, "cases of the inventory not found");
    std::fs::write(path, text).unwrap();
    println!("{written} outputs written");
}
