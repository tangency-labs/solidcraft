//! WR-63 (`cone_create_canonical_preserves_inside_out_and_checks_height` retired, part of
//! #14246): every golden row the retirement moves is a cone asked for with a semi-angle, a
//! height and `inside_out`, which upstream built outward (the request dropped) and which is
//! now the inside-out cone asked for.
//!
//! `data/wr63_moved_rows.jsonl` is the retirement's whole diff of the goldens: one line per
//! moved row, `{"unit", "case", "old", "new"}`, `old` and `new` the case's golden line before
//! and after. No other row of any unit moves (the goldens job checks every row).
//!
//! # The proof (by construction, replayed row by row)
//!
//! `Cone::create_canonical_with_semi_angle(bot, semi, Some(h), swap, inside)` builds
//! `Cone::new_bounded(bot, top, 0, 360, h, swap, a, b)` with `top = bot + tan(semi) h` (the
//! same expression), and the retirement changes only `(a, b)`: upstream's `(false, inside)`
//! (`SmCone.cpp:341-348`, `bInsideOut` dropped and passed as `bMakeNurbGenCurve`) became
//! `(inside, false)`. So only the cases with `inside == true` can move, and for each the test:
//! 1. requires the case's golden line to be exactly the inventory's `new`, and exactly the line
//!    of the same case with its cone given as the two-radius overload's `canonical {bot, top,
//!    h, swap, inside}`: the frustum from `bot` to `top` over `h` with `inside_out`, which that
//!    overload (untouched here) builds for this very request;
//! 2. requires the line of the case with its cone given as upstream built it, `cone_bounded
//!    {bot, top, 0, 360, h, swap, inside: false, make_nurb_gen: true}`, to be exactly the
//!    inventory's `old`: so the old row is the outward cone, nothing else moved, no success
//!    became a failure and no failure a success.
//!
//! It also requires the inventory to be exactly the set of these cases (a case that asks for
//! `inside` with a semi-angle and a height either moves and is listed, or gives the same line
//! both ways and is not), so no row moved for any other reason. That the inside-out cone is
//! the outward one's point set with its normal reversed and its NURB run from the top circle
//! is `surface::cone::p1`'s `wr63_semi_angle_with_height_keeps_inside_out`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::print_stdout
)] // test

use std::collections::{BTreeMap, BTreeSet};

use goldens::case::repo_root;
use goldens::{golden, registry, Case, CaseContext};
use serde_json::{json, Value};

/// The goldens' default number of generated cases per unit (the seeds they were blessed with).
const SEEDS: usize = 16;

/// The units whose cases build a cone from `surface.cone_p1`'s modes.
const UNITS: &[&str] = &["surface.cone_p1", "surface.cone_p2", "surface.surface_p2"];

/// The moved rows (the retirement's diff of the goldens).
const INVENTORY: &str = include_str!("data/wr63_moved_rows.jsonl");

/// Rewrites every `canonical_semi` mode in `v` that has a height and `inside == true` with
/// `to(bot, top, height, swap)`; the number rewritten.
fn rewrite(v: &mut Value, to: &dyn Fn(f64, f64, f64, bool) -> Value) -> usize {
    match v {
        Value::Object(m) => {
            let semi_inside = m.get("kind").and_then(Value::as_str) == Some("canonical_semi")
                && m.get("inside").and_then(Value::as_bool) == Some(true)
                && m.get("height").is_some_and(|h| !h.is_null());
            if semi_inside {
                let bot = m["bot"].as_f64().unwrap();
                let semi = m["semi"].as_f64().unwrap();
                let h = m["height"].as_f64().unwrap();
                let swap = m.get("swap").and_then(Value::as_bool).unwrap_or(false);
                // `Cone::create_canonical_with_semi_angle`'s top radius, the same expression
                let top = bot + (semi * (std::f64::consts::PI / 180.0)).tan() * h;
                *v = to(bot, top, h, swap);
                return 1;
            }
            m.values_mut().map(|x| rewrite(x, to)).sum()
        }
        Value::Array(a) => a.iter_mut().map(|x| rewrite(x, to)).sum(),
        _ => 0,
    }
}

/// `case` with its cone rewritten by `to`, or `None` when it has no such cone.
fn with_cone(case: &Case, to: &dyn Fn(f64, f64, f64, bool) -> Value) -> Option<Case> {
    let mut c = case.clone();
    (rewrite(&mut c.inputs, to) > 0).then_some(c)
}

#[test]
fn moved_rows_are_the_inside_out_cones_asked_for() {
    let mut rows: BTreeMap<String, BTreeMap<String, (Value, Value)>> = BTreeMap::new();
    for line in INVENTORY.lines() {
        let r: Value = serde_json::from_str(line).unwrap();
        let unit = r["unit"].as_str().unwrap().to_owned();
        assert!(UNITS.contains(&unit.as_str()), "a row of {unit} moved");
        let case = r["case"].as_str().unwrap().to_owned();
        let old = rows
            .entry(unit)
            .or_default()
            .insert(case, (r["old"].clone(), r["new"].clone()));
        assert!(old.is_none(), "a case listed twice");
    }
    let requested = |bot: f64, top: f64, h: f64, swap: bool| {
        json!({"kind": "canonical", "bot": bot, "top": top, "height": h, "swap": swap, "inside": true})
    };
    let upstream = |bot: f64, top: f64, h: f64, swap: bool| {
        json!({"kind": "cone_bounded", "bot": bot, "top": top, "start": 0.0, "end": 360.0,
               "height": h, "swap": swap, "inside": false, "make_nurb_gen": true})
    };
    let ctx = CaseContext {
        root: repo_root(),
        seeds: SEEDS,
    };
    let mut moved = 0usize;
    let mut unmoved = 0usize;
    for unit in registry() {
        if !UNITS.contains(&unit.id()) {
            continue;
        }
        let listed = rows.get(unit.id()).cloned().unwrap_or_default();
        let cases = unit.cases(&ctx).unwrap();
        let names: BTreeSet<&str> = cases.iter().map(|c| c.name.as_str()).collect();
        for name in listed.keys() {
            assert!(
                names.contains(name.as_str()),
                "{}: no case {name}",
                unit.id()
            );
        }
        for case in &cases {
            let Some(asked) = with_cone(case, &requested) else {
                assert!(
                    !listed.contains_key(&case.name),
                    "{} {}: moved, but asks for no inside-out cone by semi-angle and height",
                    unit.id(),
                    case.name
                );
                continue;
            };
            let built = with_cone(case, &upstream).unwrap();
            let line = |c: &Case| golden::encode(&case.name, &unit.run_rust(c).unwrap());
            let now = line(case);
            assert_eq!(
                now,
                line(&asked),
                "{} {}: not the two-radius overload's inside-out frustum",
                unit.id(),
                case.name
            );
            let before = line(&built);
            match listed.get(&case.name) {
                Some((old, new)) => {
                    assert_eq!(&now, new, "{} {}: not the new row", unit.id(), case.name);
                    assert_eq!(
                        &before,
                        old,
                        "{} {}: the old row is not upstream's outward cone",
                        unit.id(),
                        case.name
                    );
                    moved += 1;
                }
                None => {
                    assert_eq!(
                        now,
                        before,
                        "{} {}: moved but not listed",
                        unit.id(),
                        case.name
                    );
                    unmoved += 1;
                }
            }
        }
    }
    let total: usize = rows.values().map(BTreeMap::len).sum();
    assert_eq!(moved, total, "every listed row replayed");
    println!("WR-63: {moved} moved rows replayed; {unmoved} inside-out cases give the same line");
}
