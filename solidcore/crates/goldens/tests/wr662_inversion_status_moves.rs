//! WR-662 (`analytic_inversion_answers_apart` retired, part of #14246): every golden row the
//! retirement moves is the same failure as before, its `SM_ERR` (1001) now told apart as
//! `SM_ERR_NOT_WITHIN_TOLERANCE` (1021, the point is off the generator) or
//! `SM_ERR_OUTSIDE_OF_DOMAIN` (1020, the point is outside the sweep).
//!
//! Run (about the time of the three units' goldens):
//! `WR662_INVENTORY=<jsonl> cargo test --profile goldens -p goldens --test wr662_inversion_status_moves -- --ignored --nocapture`
//! where each line of the inventory is `{"unit", "case", "old", "new"}`, `old` and `new` the
//! case's golden line before and after the retirement (the PR's diff of the goldens). It is
//! `#[ignore]`d because it re-runs three whole golden units and needs the inventory; CI's
//! goldens job already checks the new rows themselves.
//!
//! # The proof (by construction, replayed row by row)
//!
//! The retirement changes no computation: `SurfOfRevolution::step_inversion` and the cone
//! branch of `Surface::step_inversion` map the error of `step_inversion_with_outputs` that
//! `is_off_generator` recognises (its generator solve found no point within the tolerance)
//! to 1021 where they returned it as 1001, and an inversion outside the sweep (`!inside`)
//! to 1020 where they returned 1001; the sphere branch maps an answered inversion off the
//! sphere to 1021. So a row moves only by those statuses. For every row of the inventory
//! this test:
//! 1. runs the case with the retired code and requires its golden line to be exactly the
//!    inventory's `new` (the code that is checked is the code that blessed);
//! 2. sets the statuses 1021 and 1020 of that output back to 1001 (all of them, else exactly
//!    one; an `offset_surface` row keeps several step operations in one digested array) and
//!    requires the golden line of the result to be exactly the inventory's `old`: topology,
//!    every other value and every other status are unchanged, so no success became a
//!    failure, no failure became a success and no value moved.
//!
//! Step 2 also pins the class: a row that moved for any other reason (for example the cone
//! intersection callers passing a failed start-plane transform on instead of reading
//! exterior, which the transform's own checks screen out for finite input) fails it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::print_stdout
)] // test

use std::collections::{BTreeMap, BTreeSet};

use goldens::case::repo_root;
use goldens::{golden, registry, CaseContext, Output};
use serde_json::Value;

/// The goldens' default number of generated cases per unit (the seeds they were blessed with).
const SEEDS: usize = 16;

/// The units whose rows the retirement moves.
const UNITS: &[&str] = &[
    "surface.offset_surface",
    "surface.surf_of_revolution_p2",
    "surface.surface_p2",
];

/// `SM_ERR`, `SM_ERR_OUTSIDE_OF_DOMAIN`, `SM_ERR_NOT_WITHIN_TOLERANCE`.
const SM_ERR: f64 = 1001.0;
const OUTSIDE_OF_DOMAIN: f64 = 1020.0;
const NOT_WITHIN_TOLERANCE: f64 = 1021.0;

/// `out` with the statuses at `at` set back to `SM_ERR`.
fn mapped(out: &Output, at: &[(String, usize)]) -> Output {
    let mut o = out.clone();
    for (key, i) in at {
        o.values.get_mut(key).unwrap()[*i] = SM_ERR;
    }
    o
}

#[test]
#[ignore = "re-runs three golden units and needs WR662_INVENTORY; see the module docs"]
fn moved_rows_are_the_same_failures_told_apart() {
    let path = std::env::var("WR662_INVENTORY").expect("WR662_INVENTORY=<jsonl>");
    let mut rows: BTreeMap<String, BTreeMap<String, (Value, Value)>> = BTreeMap::new();
    for line in std::fs::read_to_string(path).unwrap().lines() {
        let r: Value = serde_json::from_str(line).unwrap();
        let unit = r["unit"].as_str().unwrap().to_owned();
        assert!(UNITS.contains(&unit.as_str()), "a row of {unit} moved");
        let case = r["case"].as_str().unwrap().to_owned();
        rows.entry(unit)
            .or_default()
            .insert(case, (r["old"].clone(), r["new"].clone()));
    }
    let ctx = CaseContext {
        root: repo_root(),
        seeds: SEEDS,
    };
    let mut counts: BTreeMap<(String, &str), usize> = BTreeMap::new();
    let mut checked = 0usize;
    for unit in registry() {
        let Some(moved) = rows.get(unit.id()) else {
            continue;
        };
        let cases = unit.cases(&ctx).unwrap();
        let names: BTreeSet<&str> = cases.iter().map(|c| c.name.as_str()).collect();
        for name in moved.keys() {
            assert!(
                names.contains(name.as_str()),
                "{}: no case {name}",
                unit.id()
            );
        }
        for case in cases.iter().filter(|c| moved.contains_key(&c.name)) {
            let (old, new) = &moved[&case.name];
            let out = unit.run_rust(case).unwrap();
            assert_eq!(
                &golden::encode(&case.name, &out),
                new,
                "{} {}: the retired code does not give the new row",
                unit.id(),
                case.name
            );
            let told: Vec<(String, usize)> = out
                .values
                .iter()
                .flat_map(|(k, v)| {
                    v.iter()
                        .enumerate()
                        .filter(|(_, x)| **x == OUTSIDE_OF_DOMAIN || **x == NOT_WITHIN_TOLERANCE)
                        .map(move |(i, _)| (k.clone(), i))
                })
                .collect();
            let back =
                |at: &[(String, usize)]| &golden::encode(&case.name, &mapped(&out, at)) == old;
            let at: Vec<(String, usize)> = if back(&told) {
                told.clone()
            } else {
                let one: Vec<&(String, usize)> = told
                    .iter()
                    .filter(|t| back(std::slice::from_ref(*t)))
                    .collect();
                assert_eq!(
                    one.len(),
                    1,
                    "{} {}: not the old row with its told-apart statuses set back to SM_ERR",
                    unit.id(),
                    case.name
                );
                vec![one[0].clone()]
            };
            for (k, i) in &at {
                let code = if out.values[k][*i] == NOT_WITHIN_TOLERANCE {
                    "1001->1021 off the generator"
                } else {
                    "1001->1020 outside the sweep"
                };
                *counts.entry((unit.id().to_owned(), code)).or_default() += 1;
            }
            checked += 1;
        }
    }
    let total: usize = rows.values().map(BTreeMap::len).sum();
    for ((unit, code), n) in &counts {
        println!("{unit}: {n} statuses {code}");
    }
    println!("{checked} of {total} moved rows replayed: the old row with its statuses told apart");
    assert_eq!(checked, total);
}
