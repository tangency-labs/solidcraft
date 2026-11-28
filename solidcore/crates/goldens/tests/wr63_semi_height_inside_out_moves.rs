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
//!    {bot, top, 0, 360, h, swap, inside: false, make_nurb_gen: true}`, to agree with the
//!    inventory's `old` under the goldens' own rule (`golden::diff`, 1e-9 relative, the unit's
//!    floors), and `old` and `new` to disagree under it: so the old row is the outward cone,
//!    nothing else moved, no success became a failure and no failure a success.
//!
//! The `cone_bounded` mode takes the case's frame as it is, while the `CreateCanonical`
//! functions first make an `Axis2Placement` of it, so that replay is given that placement's
//! frame. Making a placement of a frame is not bit-idempotent (it renormalises and
//! re-orthogonalises), which is why the old row is compared under the goldens' rule rather
//! than bit for bit; without the placement a case with a degenerate `x` (e.g. `gen1040`)
//! would fail in the replay where the kernel succeeds.
//!
//! It also requires the inventory to be exactly the set of these cases (a case that asks for
//! `inside` with a semi-angle and a height either moves and is listed, or agrees both ways
//! and is not), so no row moved for any other reason. That the inside-out cone is
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
use serde_json::{json, Map, Value};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::math::det;
use solidcore::math::Vec3;

/// The goldens' default number of generated cases per unit (the seeds they were blessed with).
const SEEDS: usize = 16;

/// The units whose cases build a cone from `surface.cone_p1`'s modes.
const UNITS: &[&str] = &["surface.cone_p1", "surface.cone_p2", "surface.surface_p2"];

/// The moved rows (the retirement's diff of the goldens).
const INVENTORY: &str = include_str!("data/wr63_moved_rows.jsonl");

/// How a replay gives the cone: the new mode from `(bot, top, height, swap)`, and whether
/// the cone's frame is replaced by the `Axis2Placement` the `CreateCanonical` functions make
/// of it (the bounded constructor mode takes the frame as it is).
struct Replay {
    mode: fn(f64, f64, f64, bool) -> Value,
    placed: bool,
}

/// `[x, y, z]` of the cone object `cone`'s `key`.
fn v3(cone: &Map<String, Value>, key: &str) -> Vec3 {
    let a: Vec<f64> = cone[key]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect();
    Vec3::new(a[0], a[1], a[2])
}

/// Rewrites every cone object in `v` (an object with a `mode`) whose mode is
/// `canonical_semi` with a height and `inside == true` by `r`; the number rewritten.
fn rewrite(v: &mut Value, r: &Replay) -> usize {
    match v {
        Value::Object(m) => {
            let semi_inside = m.get("mode").is_some_and(|mode| {
                mode.get("kind").and_then(Value::as_str) == Some("canonical_semi")
                    && mode.get("inside").and_then(Value::as_bool) == Some(true)
                    && mode.get("height").is_some_and(|h| !h.is_null())
            });
            if semi_inside {
                let mode = &m["mode"];
                let bot = mode["bot"].as_f64().unwrap();
                let semi = mode["semi"].as_f64().unwrap();
                let h = mode["height"].as_f64().unwrap();
                let swap = mode.get("swap").and_then(Value::as_bool).unwrap_or(false);
                // `Cone::create_canonical_with_semi_angle`'s top radius, the same expression
                let top = bot + det::tan(semi * (std::f64::consts::PI / 180.0)) * h;
                m.insert("mode".to_owned(), (r.mode)(bot, top, h, swap));
                if r.placed {
                    let p = Axis2Placement::new(v3(m, "origin"), v3(m, "x"), v3(m, "y"));
                    for (key, a) in [("origin", p.origin()), ("x", p.x_axis()), ("y", p.y_axis())] {
                        m.insert(key.to_owned(), json!([a.x, a.y, a.z]));
                    }
                }
                return 1;
            }
            m.values_mut().map(|x| rewrite(x, r)).sum()
        }
        Value::Array(a) => a.iter_mut().map(|x| rewrite(x, r)).sum(),
        _ => 0,
    }
}

/// `case` with its cone rewritten by `r`, or `None` when it has no such cone.
fn with_cone(case: &Case, r: &Replay) -> Option<Case> {
    let mut c = case.clone();
    (rewrite(&mut c.inputs, r) > 0).then_some(c)
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
    // the two-radius overload, through the same placement as the semi-angle one
    let requested = Replay {
        mode: |bot, top, h, swap| {
            json!({"kind": "canonical", "bot": bot, "top": top, "height": h, "swap": swap,
                   "inside": true})
        },
        placed: false,
    };
    // upstream's `new SmCone(.., FALSE /* bInsideOut */, bInsideOut /* bMakeNurbGenCurve */)`
    let upstream = Replay {
        mode: |bot, top, h, swap| {
            json!({"kind": "cone_bounded", "bot": bot, "top": top, "start": 0.0, "end": 360.0,
                   "height": h, "swap": swap, "inside": false, "make_nurb_gen": true})
        },
        placed: true,
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
            let agree = |a: &Value, b: &Value| golden::diff(a, b, unit.abs_floors()).is_none();
            if let Some((old, new)) = listed.get(&case.name) {
                assert_eq!(&now, new, "{} {}: not the new row", unit.id(), case.name);
                assert!(
                    agree(old, &before),
                    "{} {}: the old row is not upstream's outward cone: {old} vs {before}",
                    unit.id(),
                    case.name
                );
                assert!(
                    !agree(old, new),
                    "{} {}: listed, but old and new agree",
                    unit.id(),
                    case.name
                );
                moved += 1;
            } else {
                assert!(
                    agree(&before, &now),
                    "{} {}: moved but not listed ({}): {before} vs {now}",
                    unit.id(),
                    case.name,
                    case.inputs
                );
                unmoved += 1;
            }
        }
    }
    let total: usize = rows.values().map(BTreeMap::len).sum();
    assert_eq!(moved, total, "every listed row replayed");
    println!("WR-63: {moved} moved rows replayed; {unmoved} inside-out cases give the same line");
}
