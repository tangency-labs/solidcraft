//! #17856's substitution replay of the golden rows WR-57's tiebreak moved downstream.
//!
//! #17856 changes what [`Surface::find_degen_param_for_direction`] and the singular point
//! inversion ([`Surface::invert_point_near_singularity`]) answer. Every other row it moved
//! is downstream of those answers. The `wr57-replay` seam (`solidcore`'s
//! `surface::surface::replay`, a feature this crate turns on for its tests only) sees each
//! outermost call of the two that a runner's kernel makes, numbered in call order, with
//! its inputs, surface and answer. The seam is the kernel's
//! ([`solidcore::Kernel::replay`]); the runners make their kernels with
//! [`crate::runner_kernel`], which gives each the seam a test here installs on its thread
//! ([`with_seam`]).
//!
//! For each moved row, [`DATA`] holds, from main's run of the case ([`MAIN_COMMIT`]) and
//! this branch's:
//! - the FNV-1a hash of main's encoded row;
//! - main's call count, and a hash of main's call sequence (seam, inputs and a digest of
//!   the surface, call by call: [`fold_call`]);
//! - every call whose answer differs, as `(call, main's answer)`, and a hash of this
//!   branch's answers at those calls.
//!
//! The tests run each case twice:
//! 1. with main's answer put in place at exactly those calls (this branch's own answers
//!    there must be the taped ones; every call's seam, inputs and surface, and the count,
//!    must be main's), the encoded row hashes to main's row, and each substituted call is
//!    certified (`cert`): this branch's answer is proven no worse than main's, or main's
//!    is proven wrong, with the rounding charged; the classes are counted per unit and the
//!    counts asserted;
//! 2. as is, the encoded row equals the committed golden line.
//!
//! So each committed row is this branch's output, and it differs from main's run only
//! through the listed answers of the two functions, each certified. A third test runs
//! every row without the tape: no row is then main's row.
//!
//! [`DATA`] was made by [`record`] on main ([`MAIN_COMMIT`], with the same seam wrapped
//! around main's two functions) and then on this branch fed main's log (`WR57_MAIN`),
//! which replays main's answers at every call and writes the calls whose answers differ.
//!
//! **When a later change moves one of these rows** (or the calls a row makes): the tape
//! is #17856's proof against [`MAIN_COMMIT`] and is not kept in step with later code. A
//! change that re-blesses a row listed in [`DATA`], or changes its calls, either records
//! the tape again (the [`record`] steps above, on the main it merges into, as #17856 did)
//! or deletes that row's [`DATA`] entry and lowers the unit's row count in the tests
//! below. Each failure message names [`MAIN_COMMIT`].
//!
//! [`Surface::find_degen_param_for_direction`]: solidcore::surface::surface::Surface::find_degen_param_for_direction
//! [`Surface::invert_point_near_singularity`]: solidcore::surface::surface::Surface::invert_point_near_singularity

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use solidcore::surface::surface::replay::{self, Answer, Call, ReplaySeam, Seam};
use solidcore::surface::surface::Surface;
use solidcore::{Kernel, SolidError};

use crate::case::{repo_root, Case, CaseContext};
use crate::golden::{default_dir, encode, read};
use crate::rng::fnv1a64;
use crate::UnitDiff;

mod cert;
mod data;
use data::DATA;

/// The main commit [`DATA`] was recorded on.
pub(super) const MAIN_COMMIT: &str = "b76b3fd2b";

/// Generated cases per unit, as `goldens` runs them (`main.rs` `DEFAULT_SEEDS`).
const SEEDS: usize = 16;

thread_local! {
    /// The seam a test here installed on this thread, for [`with_seam`].
    static SEAM: RefCell<Option<ReplaySeam>> = const { RefCell::new(None) };
}

/// `k` with the seam a test here installed on this thread, if any ([`crate::runner_kernel`]).
pub(crate) fn with_seam(mut k: Kernel) -> Kernel {
    SEAM.with(|s| {
        if let Some(seam) = s.borrow().as_ref() {
            k.replay = seam.clone();
        }
    });
    k
}

/// Runs `f` with `hook` installed as this thread's seam.
fn with_hook<R>(hook: impl FnMut(Call<'_>) -> Answer + Send + 'static, f: impl FnOnce() -> R) -> R {
    SEAM.with(|s| *s.borrow_mut() = Some(ReplaySeam::new(Box::new(hook))));
    let r = f();
    SEAM.with(|s| *s.borrow_mut() = None);
    r
}

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

/// A digest of `s`: its natural domain's bits and its points at the domain's corners and
/// centre, evaluated on a kernel of their own (so the runner's kernel is not touched).
fn surface_digest(s: &Surface) -> u64 {
    use solidcore::geom::vector2d::Point2;
    let k = Kernel::default();
    let Ok(d) = s.natural_uv_domain_with_nurb(&k) else {
        return fold(FNV_OFFSET, &[u64::MAX]);
    };
    let (a, b) = (d.min(), d.max());
    let mut h = fold(
        FNV_OFFSET,
        &[a.x.to_bits(), a.y.to_bits(), b.x.to_bits(), b.y.to_bits()],
    );
    let mid = Point2::new(0.5 * (a.x + b.x), 0.5 * (a.y + b.y));
    for uv in [a, Point2::new(a.x, b.y), Point2::new(b.x, a.y), b, mid] {
        h = match s.evaluate_point(&k, uv) {
            Ok(p) => fold(h, &[0, p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]),
            Err(_) => fold(h, &[1]),
        };
    }
    h
}

/// Folds one call (seam, input count, inputs, the surface's digest) into `h`.
fn fold_call(h: u64, seam: Seam, input: &[u64], surface: u64) -> u64 {
    let h = fold(h, &[u64::from(seam == Seam::Invert), input.len() as u64]);
    fold(fold(h, input), &[surface])
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
#[derive(Default)]
struct Seen {
    calls: u32,
    call_hash: u64,
    now_hash: u64,
    applied: usize,
    /// The certificate classes of the substituted calls (`cert`), when certified.
    classes: BTreeMap<String, usize>,
}

/// What makes two substituted calls the same for their certificate: whether the seam is
/// the inversion, the inputs' bits, the surface, maximize flag, tolerances and fixes as
/// text, and main's and this branch's answers' bits (`None` for an error).
type CallKey = (bool, Vec<u64>, String, Option<Vec<u64>>, Option<Vec<u64>>);

/// Runs `case` with main's answers `subs` put in place (each certified when `certify`);
/// returns the encoded row and what the hook saw.
fn substituted(u: &dyn UnitDiff, case: &Case, subs: &[(u32, Ans)], certify: bool) -> (Value, Seen) {
    let seen = Arc::new(Mutex::new(Seen {
        call_hash: FNV_OFFSET,
        now_hash: FNV_OFFSET,
        ..Seen::default()
    }));
    let tape: BTreeMap<u32, Ans> = subs.iter().copied().collect();
    // (a call repeated with the same seam, inputs, surface geometry (its kind's `Debug`,
    // every value to the bit), maximize flag, tolerances, fixes and both answers has the
    // same class: certified once; an error answer is classed by being one)
    let mut known: BTreeMap<CallKey, String> = BTreeMap::new();
    let s = Arc::clone(&seen);
    let line = with_hook(
        move |c: Call<'_>| {
            let mut s = s.lock().unwrap();
            let i = s.calls;
            s.calls += 1;
            let sd = surface_digest(c.surface);
            s.call_hash = fold_call(s.call_hash, c.seam, c.input, sd);
            match tape.get(&i) {
                Some(&main) => {
                    s.now_hash = fold_answer(s.now_hash, &c.own);
                    s.applied += 1;
                    let main = to_answer(main);
                    if certify {
                        let key = (
                            c.seam == Seam::Invert,
                            c.input.to_vec(),
                            format!(
                                "{:?} {} {:?} {:?}",
                                c.surface.kind(),
                                c.farthest,
                                c.kernel.tolerances,
                                c.kernel.fixes
                            ),
                            main.as_ref().ok().cloned(),
                            c.own.as_ref().ok().cloned(),
                        );
                        let class = known
                            .entry(key)
                            .or_insert_with(|| cert::classify(&c, &main))
                            .clone();
                        *s.classes.entry(class).or_default() += 1;
                    }
                    main
                }
                None => c.own,
            }
        },
        || run(u, case),
    );
    let seen = std::mem::take(&mut *seen.lock().unwrap());
    (line, seen)
}

/// Why the replay of `row` with `subs` in place fails, if it does (with the substituted
/// calls certified when `certify`), and the certificate classes.
fn check(
    u: &dyn UnitDiff,
    case: &Case,
    row: &Row,
    subs: &[(u32, Ans)],
    certify: bool,
) -> (Vec<String>, BTreeMap<String, usize>) {
    let &(_, name, main_hash, calls, call_hash, now_hash, _) = row;
    let (line, seen) = substituted(u, case, subs, certify);
    let mut failures = Vec::new();
    if seen.calls != calls || seen.call_hash != call_hash {
        failures.push(format!(
            "{name}: {} of {calls} calls, call sequence {} (the tape is main {MAIN_COMMIT}'s)",
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
        failures.push(format!(
            "{name}: substituted row is not main {MAIN_COMMIT}'s row"
        ));
    }
    (failures, seen.classes)
}

/// Runs every row of `unit_id` in [`DATA`] substituted (certifying each substituted call)
/// and as is; returns the row count and the certificate classes over the unit's rows.
fn replay_unit(unit_id: &str) -> (usize, BTreeMap<String, usize>) {
    let u = unit(unit_id);
    let all = cases(u.as_ref());
    let golden = read(&default_dir(), unit_id).unwrap();
    let mut failures = Vec::new();
    let mut classes: BTreeMap<String, usize> = BTreeMap::new();
    let mut n = 0;
    for row in DATA.iter().filter(|r| r.0 == unit_id) {
        n += 1;
        let (name, subs) = (row.1, row.6);
        let case = all
            .get(name)
            .unwrap_or_else(|| panic!("{unit_id}: no case {name}"));
        assert!(!subs.is_empty(), "{unit_id} {name}: no substitution");
        let (f, c) = check(u.as_ref(), case, row, subs, true);
        failures.extend(f);
        for (class, count) in c {
            if class.starts_with("FAULT") {
                failures.push(format!("{name}: {count} calls {class}"));
            }
            *classes.entry(class).or_default() += count;
        }
        let line = run(u.as_ref(), case);
        if golden.get(name) != Some(&line) {
            failures.push(format!("{name}: the row is not the committed golden line"));
        }
    }
    assert!(
        failures.is_empty(),
        "{unit_id} (tape of main {MAIN_COMMIT}; see the module doc when a change moves \
         these rows):\n{}",
        failures.join("\n")
    );
    (n, classes)
}

macro_rules! replay_tests {
    ($($test:ident: $unit:literal, $rows:literal, $classes:expr;)*) => {$(
        #[test]
        fn $test() {
            let (rows, classes) = replay_unit($unit);
            eprintln!("{}: {rows} rows, certificate classes {classes:#?}", $unit);
            assert_eq!(rows, $rows, "the rows of {}", $unit);
            let want: BTreeMap<String, usize> =
                $classes.iter().map(|&(c, n): &(&str, usize)| (c.to_string(), n)).collect();
            assert_eq!(classes, want, "the certificate classes of {}", $unit);
        }
    )*};
}

replay_tests! {
    fillet_executive_p3_rows_move_only_through_the_wr57_answers: "blend.fillet_executive_p3", 16, cert::EXECUTIVE_P3;
    fillet_geom_p2_rows_move_only_through_the_wr57_answers: "blend.fillet_geom_p2", 1, cert::GEOM_P2;
    fillet_standard_solver_p1_rows_move_only_through_the_wr57_answers: "blend.fillet_standard_solver_p1", 1, cert::STANDARD_SOLVER_P1;
    heal_data_p3_rows_move_only_through_the_wr57_answers: "heal.heal_data_p3", 5, cert::HEAL_DATA_P3;
    surface_drop_lib_p3_rows_move_only_through_the_wr57_answers: "intersect.surface_drop_lib_p3", 1, cert::DROP_LIB_P3;
    api_queries_rows_move_only_through_the_wr57_answers: "query.api_queries", 1, cert::API_QUERIES;
    edgeuse_p1_rows_move_only_through_the_wr57_answers: "topology.edgeuse_p1", 1, cert::EDGEUSE_P1;
    edgeuse_p2_rows_move_only_through_the_wr57_answers: "topology.edgeuse_p2", 8, cert::EDGEUSE_P2;
    edgeuse_p3_rows_move_only_through_the_wr57_answers: "topology.edgeuse_p3", 13, cert::EDGEUSE_P3;
    vertex_rows_move_only_through_the_wr57_answers: "topology.vertex", 1, cert::VERTEX;
    vertexuse_rows_move_only_through_the_wr57_answers: "topology.vertexuse", 4, cert::VERTEXUSE;
}

/// A unit's runner and its cases by name.
type Loaded = (Box<dyn UnitDiff>, BTreeMap<String, Case>);

/// The revert run: without the tape (this branch as is) no row hashes to main's row (the
/// row itself fails, not only the tape's bookkeeping). The tape is what makes each row
/// main's.
///
/// Partial perturbations are not row-level tests: on 41 of the 52 rows the last entry
/// is dropped or moved to the next call and the encoded row is still main's (that
/// answer does not reach it). On 5 `fillet_executive_p3` rows, moving every entry one
/// call later also leaves main's row. Both were measured on #17856's r7 tape.
#[test]
fn without_the_tape_no_row_is_mains() {
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
        assert!(!subs.is_empty(), "{} {name}: no substitution", row.0);
        let (line, _) = substituted(u.as_ref(), case, &[], false);
        if hash(&line) == row.2 {
            failures.push(format!("{} {name}: main's row without the tape", row.0));
        }
    }
    assert!(
        failures.is_empty(),
        "(tape of main {MAIN_COMMIT})\n{}",
        failures.join("\n")
    );
    assert_eq!(DATA.len(), 52);
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
/// row hash, the encoded row and every call's seam, inputs, surface digest and answer.
/// With `$WR57_MAIN` (that output of main's run) it answers main's answer at every call
/// instead, after checking the call's seam, inputs and surface are main's, and writes the
/// calls whose own answer differs.
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
        let log = Arc::new(Mutex::new(Vec::<Value>::new()));
        let l2 = Arc::clone(&log);
        let line = with_hook(
            move |c: Call<'_>| {
                let mut log = l2.lock().unwrap();
                let i = log.len();
                let enc = |a: &Answer| match a {
                    Ok(b) => json!({ "ok": b }),
                    Err(e) => json!({ "err": format!("{e:?}"), "msg": e.to_string() }),
                };
                let call = json!({
                    "seam": if c.seam == Seam::Invert { "invert" } else { "degen" },
                    "in": c.input, "sd": surface_digest(c.surface), "a": enc(&c.own),
                });
                if !probe {
                    log.push(call);
                    return c.own;
                }
                let Some(m) = main_calls.get(i) else {
                    log.push(json!({ "diverged": "more calls than main", "call": call }));
                    return c.own;
                };
                if m["seam"] != call["seam"] || m["in"] != call["in"] || m["sd"] != call["sd"] {
                    log.push(
                        json!({ "diverged": "seam, inputs or surface", "call": call, "main": m }),
                    );
                    return c.own;
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
                    None if ma == &call["a"] => c.own,
                    None => Err(rebuild(ma["err"].as_str().unwrap())),
                }
            },
            || run(u.as_ref(), case),
        );
        let calls = std::mem::take(&mut *log.lock().unwrap());
        let rec = json!({
            "unit": id, "case": name, "hash": hash(&line).to_string(),
            "line": line, "calls": calls,
        });
        let _ = writeln!(out, "{rec}");
    }
    std::fs::write(std::env::var("WR57_OUT").unwrap(), out).unwrap();
}
