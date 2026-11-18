//! #17856's substitution replay of the golden rows WR-57's tiebreak moved downstream.
//!
//! #17856 changes what [`Surface::find_degen_param_for_direction`] and the singular point
//! inversion ([`Surface::invert_point_near_singularity`]) answer. Every other row it moved
//! is downstream of those answers. The `wr57-replay` seam (`solidcore`'s
//! `surface::surface::replay`, a feature this crate turns on for its tests only) sees each
//! outermost call of the two, numbered in call order, with its inputs and answer.
//!
//! For each moved row, [`DATA`] holds, from main's run of the case (`bf685d1b3`) and this
//! branch's:
//! - the FNV-1a hash of main's encoded row;
//! - main's call count, and a hash of main's call sequence (seam and inputs, call by call);
//! - every call whose answer differs, as `(call, main's answer)`, and a hash of this
//!   branch's answers at those calls.
//!
//! The tests run each case twice:
//! 1. with main's answer put in place at exactly those calls (this branch's own answers
//!    there must be the taped ones; every call's seam and inputs, and the count, must be
//!    main's), the encoded row hashes to main's row;
//! 2. as is, the encoded row equals the committed golden line.
//!
//! So each committed row is this branch's output, and it differs from main's run only
//! through the listed answers of the two functions. A third test perturbs the tape (no
//! entry, the last entry dropped or moved to the next call) and the replay must then fail.
//!
//! [`DATA`] was made by [`record`] on main (with the same seam on main's two functions)
//! and then on this branch fed main's log (`WR57_MAIN`), which replays main's answers at
//! every call and writes the calls whose answers differ.
//!
//! [`Surface::find_degen_param_for_direction`]: solidcore::surface::surface::Surface::find_degen_param_for_direction
//! [`Surface::invert_point_near_singularity`]: solidcore::surface::surface::Surface::invert_point_near_singularity

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use serde_json::{json, Value};
use solidcore::surface::surface::replay::{self, Answer, Seam};
use solidcore::SolidError;

use crate::case::{repo_root, Case, CaseContext};
use crate::golden::{default_dir, encode, read};
use crate::rng::fnv1a64;
use crate::UnitDiff;

mod data;
use data::DATA;

/// Generated cases per unit, as `goldens` runs them (`main.rs` `DEFAULT_SEEDS`).
const SEEDS: usize = 16;

/// A taped answer of main's: the value's three words ([`replay::Bits`]), or the error.
#[derive(Clone, Copy)]
pub(super) enum Ans {
    /// The value's words.
    Ok(u64, u64, u64),
    /// The error.
    Err(fn() -> SolidError),
}

/// One moved row: unit, case, main's row hash, main's call count, main's call sequence
/// hash ([`fold_call`]), the hash of this branch's answers at the substituted calls
/// ([`fold_answer`]) and the substituted calls with main's answer.
pub(super) type Row = (
    &'static str,
    &'static str,
    u64,
    u32,
    u64,
    u64,
    &'static [(u32, Ans)],
);

fn unit(id: &str) -> Box<dyn UnitDiff> {
    crate::registry()
        .into_iter()
        .find(|u| u.id() == id)
        .unwrap_or_else(|| panic!("no unit {id}"))
}

fn cases(u: &dyn UnitDiff) -> BTreeMap<String, Case> {
    let ctx = CaseContext {
        root: repo_root(),
        seeds: SEEDS,
    };
    u.cases(&ctx)
        .unwrap()
        .into_iter()
        .map(|c| (c.name.clone(), c))
        .collect()
}

fn run(u: &dyn UnitDiff, case: &Case) -> Value {
    let out = u.run_rust(case);
    encode(
        &case.name,
        &out.unwrap_or_else(|e| panic!("{}: {e:?}", case.name)),
    )
}

fn hash(v: &Value) -> u64 {
    fnv1a64(&serde_json::to_string(v).unwrap())
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a over 64-bit words.
fn fold(mut h: u64, words: &[u64]) -> u64 {
    for w in words {
        h ^= w;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Folds one call (seam, input count, inputs) into `h`.
fn fold_call(h: u64, seam: Seam, input: &[u64]) -> u64 {
    let h = fold(h, &[u64::from(seam == Seam::Invert), input.len() as u64]);
    fold(h, input)
}

/// Folds one answer (its words, or its error's message) into `h`.
fn fold_answer(h: u64, a: &Answer) -> u64 {
    match a {
        Ok(words) => fold(fold(h, &[0, words.len() as u64]), words),
        Err(e) => fold(h, &[1, fnv1a64(&e.to_string())]),
    }
}

fn to_answer(a: Ans) -> Answer {
    match a {
        Ans::Ok(x, y, z) => Ok(vec![x, y, z]),
        Ans::Err(e) => Err(e()),
    }
}

/// What a substituted run saw.
struct Seen {
    calls: u32,
    call_hash: u64,
    now_hash: u64,
    applied: usize,
}

/// Runs `case` with main's answers `subs` put in place; returns the encoded row and what
/// the hook saw.
fn substituted(u: &dyn UnitDiff, case: &Case, subs: &[(u32, Ans)]) -> (Value, Seen) {
    let seen = Rc::new(RefCell::new(Seen {
        calls: 0,
        call_hash: FNV_OFFSET,
        now_hash: FNV_OFFSET,
        applied: 0,
    }));
    let tape: BTreeMap<u32, Ans> = subs.iter().copied().collect();
    let s = Rc::clone(&seen);
    replay::install(Some(Box::new(move |seam, input, own| {
        let mut s = s.borrow_mut();
        let i = s.calls;
        s.calls += 1;
        s.call_hash = fold_call(s.call_hash, seam, input);
        match tape.get(&i) {
            Some(&main) => {
                s.now_hash = fold_answer(s.now_hash, &own);
                s.applied += 1;
                to_answer(main)
            }
            None => own,
        }
    })));
    let line = run(u, case);
    replay::install(None);
    let seen = Rc::try_unwrap(seen).ok().unwrap().into_inner();
    (line, seen)
}

/// Why the replay of `row` with `subs` in place fails, if it does.
fn check(u: &dyn UnitDiff, case: &Case, row: &Row, subs: &[(u32, Ans)]) -> Vec<String> {
    let &(_, name, main_hash, calls, call_hash, now_hash, _) = row;
    let (line, seen) = substituted(u, case, subs);
    let mut failures = Vec::new();
    if seen.calls != calls || seen.call_hash != call_hash {
        failures.push(format!(
            "{name}: {} of {calls} calls, call sequence {}",
            seen.calls,
            if seen.call_hash == call_hash {
                "main's"
            } else {
                "not main's"
            }
        ));
    }
    if seen.applied != subs.len() || seen.now_hash != now_hash {
        failures.push(format!(
            "{name}: {} of {} substitutions, this branch's answers there {}",
            seen.applied,
            subs.len(),
            if seen.now_hash == now_hash {
                "as taped"
            } else {
                "not as taped"
            }
        ));
    }
    if hash(&line) != main_hash {
        failures.push(format!("{name}: substituted row is not main's row"));
    }
    failures
}

/// Runs every row of `unit_id` in [`DATA`] substituted and as is; returns the row count.
fn replay_unit(unit_id: &str) -> usize {
    let u = unit(unit_id);
    let all = cases(u.as_ref());
    let golden = read(&default_dir(), unit_id).unwrap();
    let mut failures = Vec::new();
    let mut n = 0;
    for row in DATA.iter().filter(|r| r.0 == unit_id) {
        n += 1;
        let (name, subs) = (row.1, row.6);
        let case = all
            .get(name)
            .unwrap_or_else(|| panic!("{unit_id}: no case {name}"));
        assert!(!subs.is_empty(), "{unit_id} {name}: no substitution");
        failures.extend(check(u.as_ref(), case, row, subs));
        let line = run(u.as_ref(), case);
        if golden.get(name) != Some(&line) {
            failures.push(format!("{name}: the row is not the committed golden line"));
        }
    }
    assert!(failures.is_empty(), "{unit_id}:\n{}", failures.join("\n"));
    n
}

macro_rules! replay_tests {
    ($($test:ident: $unit:literal, $rows:literal;)*) => {$(
        #[test]
        fn $test() {
            assert_eq!(replay_unit($unit), $rows);
        }
    )*};
}

replay_tests! {
    fillet_executive_p3_rows_move_only_through_the_wr57_answers: "blend.fillet_executive_p3", 16;
    fillet_geom_p2_rows_move_only_through_the_wr57_answers: "blend.fillet_geom_p2", 1;
    fillet_standard_solver_p1_rows_move_only_through_the_wr57_answers: "blend.fillet_standard_solver_p1", 1;
    surface_drop_lib_p3_rows_move_only_through_the_wr57_answers: "intersect.surface_drop_lib_p3", 1;
    api_queries_rows_move_only_through_the_wr57_answers: "query.api_queries", 1;
    edgeuse_p1_rows_move_only_through_the_wr57_answers: "topology.edgeuse_p1", 1;
    edgeuse_p2_rows_move_only_through_the_wr57_answers: "topology.edgeuse_p2", 8;
    edgeuse_p3_rows_move_only_through_the_wr57_answers: "topology.edgeuse_p3", 13;
    face_p3_rows_move_only_through_the_wr57_answers: "topology.face_p3", 8;
    vertex_rows_move_only_through_the_wr57_answers: "topology.vertex", 1;
    vertexuse_rows_move_only_through_the_wr57_answers: "topology.vertexuse", 4;
}

/// A unit's runner and its cases by name.
type Loaded = (Box<dyn UnitDiff>, BTreeMap<String, Case>);

/// The revert run: the replay fails when the tape is perturbed. On every row: with no
/// substitution (this branch as is) the row is not main's; with the last entry dropped,
/// or moved to the next call, the replay fails.
#[test]
fn a_perturbed_tape_fails_the_replay() {
    let mut by_unit: BTreeMap<&str, Loaded> = BTreeMap::new();
    let mut failures = Vec::new();
    for row in DATA {
        let (u, all) = by_unit.entry(row.0).or_insert_with(|| {
            let u = unit(row.0);
            let all = cases(u.as_ref());
            (u, all)
        });
        let (name, subs) = (row.1, row.6);
        let case = &all[name];
        let (line, _) = substituted(u.as_ref(), case, &[]);
        if hash(&line) == row.2 {
            failures.push(format!("{} {name}: main's row with no substitution", row.0));
        }
        let fewer = &subs[..subs.len() - 1];
        if check(u.as_ref(), case, row, fewer).is_empty() {
            failures.push(format!(
                "{} {name}: passes with the last entry dropped",
                row.0
            ));
        }
        let mut moved = subs.to_vec();
        if let Some(e) = moved.last_mut() {
            e.0 += 1;
        }
        if check(u.as_ref(), case, row, &moved).is_empty() {
            failures.push(format!(
                "{} {name}: passes with the last entry moved",
                row.0
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(DATA.len(), 55);
}

/// Main's error from its `Debug` text: a surface routine's `Failed` phrase (the only kind
/// main answers where this branch answers otherwise).
fn rebuild(debug: &str) -> SolidError {
    let field = |key: &str| {
        let at = debug.find(key).unwrap() + key.len();
        let end = at + debug[at..].find('"').unwrap();
        &*Box::leak(debug[at..end].to_string().into_boxed_str())
    };
    let e = replay::surface_failed(field("operation: \""), field("Surface(Failed(\""));
    assert_eq!(format!("{e:?}"), debug, "main's error is rebuilt as it was");
    e
}

/// Writes, per `unit<TAB>case` line of `$WR57_CASES`, one JSON line to `$WR57_OUT`: the
/// row hash, the encoded row and every call's seam, inputs and answer. With `$WR57_MAIN`
/// (that output of main's run) it answers main's answer at every call instead, after
/// checking the call's seam and inputs are main's, and writes the calls whose own answer
/// differs.
#[test]
#[ignore = "makes the replay's data; run on main, then on this branch with WR57_MAIN"]
fn record() {
    use std::fmt::Write as _;
    let list = std::fs::read_to_string(std::env::var("WR57_CASES").unwrap()).unwrap();
    let main: BTreeMap<(String, String), Value> = match std::env::var("WR57_MAIN") {
        Ok(p) => std::fs::read_to_string(p)
            .unwrap()
            .lines()
            .map(|l| {
                let v: Value = serde_json::from_str(l).unwrap();
                let key = (
                    v["unit"].as_str().unwrap().to_string(),
                    v["case"].as_str().unwrap().to_string(),
                );
                (key, v)
            })
            .collect(),
        Err(_) => BTreeMap::new(),
    };
    let probe = !main.is_empty();
    let mut by_unit: BTreeMap<String, BTreeMap<String, Case>> = BTreeMap::new();
    let mut out = String::new();
    for l in list.lines() {
        let (id, name) = l.split_once('\t').unwrap();
        let u = unit(id);
        let all = by_unit
            .entry(id.to_string())
            .or_insert_with(|| cases(u.as_ref()));
        let case = all
            .get(name)
            .unwrap_or_else(|| panic!("{id}: no case {name}"));
        let main_calls: Vec<Value> = main
            .get(&(id.to_string(), name.to_string()))
            .map(|v| v["calls"].as_array().unwrap().clone())
            .unwrap_or_default();
        let log = Rc::new(RefCell::new(Vec::<Value>::new()));
        let l2 = Rc::clone(&log);
        replay::install(Some(Box::new(move |seam, input, own| {
            let mut log = l2.borrow_mut();
            let i = log.len();
            let enc = |a: &Answer| match a {
                Ok(b) => json!({ "ok": b }),
                Err(e) => json!({ "err": format!("{e:?}"), "msg": e.to_string() }),
            };
            let call = json!({
                "seam": if seam == Seam::Invert { "invert" } else { "degen" },
                "in": input, "a": enc(&own),
            });
            if !probe {
                log.push(call);
                return own;
            }
            let Some(m) = main_calls.get(i) else {
                log.push(json!({ "diverged": "more calls than main", "call": call }));
                return own;
            };
            if m["seam"] != call["seam"] || m["in"] != call["in"] {
                log.push(json!({ "diverged": "seam or inputs", "call": call, "main": m }));
                return own;
            }
            let ma = &m["a"];
            log.push(json!({ "i": i, "now": call["a"], "main": ma }));
            match ma.get("ok") {
                Some(b) => Ok(b
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_u64().unwrap())
                    .collect()),
                // (an error main answered: keep this branch's when it is the same, else
                // rebuild it from its operation and phrase, as `data.rs` does)
                None if ma == &call["a"] => own,
                None => Err(rebuild(ma["err"].as_str().unwrap())),
            }
        })));
        let line = run(u.as_ref(), case);
        replay::install(None);
        let calls = Rc::try_unwrap(log).ok().unwrap().into_inner();
        let rec = json!({
            "unit": id, "case": name, "hash": hash(&line).to_string(),
            "line": line, "calls": calls,
        });
        let _ = writeln!(out, "{rec}");
    }
    std::fs::write(std::env::var("WR57_OUT").unwrap(), out).unwrap();
}

/// `topology.vertex` `surf_sphere150_rebuild_v3_gaps` (its weighted sum 243 -> 242): the
/// row moves through call 4 alone, a `find_degen_param_for_direction` at the pole whose
/// `u` this branch answers 3 ulps below main's (15.346539759579205 against
/// 15.34653975957921). With main's answer at call 4 only, the row is main's; with main's
/// answer at every other call, it is this branch's committed line. (At this branch's `u`
/// the vertex/face gap of vertex use 2 evaluates to 8.9e-16 instead of 0, so
/// `GetMaxUpDimGap3d` takes it as the larger; the gap reported is 0.0 either way, a face
/// gap of vertex use 1 instead of the edge gap of vertex use 2.)
#[test]
fn the_vertex_row_moves_through_call_4_alone() {
    let row = DATA
        .iter()
        .find(|r| r.1 == "surf_sphere150_rebuild_v3_gaps")
        .unwrap();
    let u = unit(row.0);
    let case = &cases(u.as_ref())[row.1];
    let (only, others): (Vec<_>, Vec<_>) = row.6.iter().partition(|e| e.0 == 4);
    assert_eq!(only.len(), 1);
    let (line, _) = substituted(u.as_ref(), case, &only);
    assert_eq!(hash(&line), row.2, "call 4 alone gives main's row");
    let (line, _) = substituted(u.as_ref(), case, &others);
    let golden = read(&default_dir(), row.0).unwrap();
    assert_eq!(
        golden.get(row.1),
        Some(&line),
        "the others leave this branch's row"
    );
    let Ans::Ok(now_u, ..) = to_ans_now(u.as_ref(), case) else {
        panic!("call 4 answers a value");
    };
    let Ans::Ok(main_u, ..) = only[0].1 else {
        panic!("main's call 4 is a value");
    };
    assert_eq!(f64::from_bits(now_u), 15.346539759579205);
    assert_eq!(f64::from_bits(main_u), 15.34653975957921);
    assert_eq!(main_u - now_u, 3);
}

/// This branch's own answer at call 4 of `case`.
fn to_ans_now(u: &dyn UnitDiff, case: &Case) -> Ans {
    let got = Rc::new(RefCell::new(None));
    let g = Rc::clone(&got);
    let mut n = 0u32;
    replay::install(Some(Box::new(move |_, _, own| {
        if n == 4 {
            if let Ok(w) = &own {
                *g.borrow_mut() = Some(Ans::Ok(w[0], w[1], w[2]));
            }
        }
        n += 1;
        own
    })));
    let _ = u.run_rust(case);
    replay::install(None);
    let got = got.borrow_mut().take();
    got.unwrap()
}
