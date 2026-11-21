//! Atomic command tests: the engine's transaction contract, checked on random designs.
//!
//! The contract (crates/engine/src/lib.rs, `Session::execute`):
//! 1. A command that returns an error leaves the design exactly as it was.
//! 2. Undo back to the start gives the empty design. Redo back to the end gives the final design.
//! 3. The same commands give the same design every time (no hidden state between runs).
//!
//! The designs come from the engine's own fuzzer (`fuzz::run`), so these tests cover every
//! command it can generate: sketches, extrudes, revolves, holes, patterns, fillets, shells.
//!
//! Seeds: `ATOMIC_SEEDS` (default 0..4). Pull requests skip this file (.config/nextest.toml); pushes
//! to main run it, and the nightly `property-tests` job uses more seeds. A long local run:
//! `ATOMIC_SEEDS=0..100`.

use solidcraft_doc::Document;
use solidcraft_engine::{Session, fuzz};

fn seeds() -> std::ops::Range<u64> {
    let s = std::env::var("ATOMIC_SEEDS").unwrap_or_else(|_| "0..4".into());
    let (a, b) = s.split_once("..").expect("ATOMIC_SEEDS looks like 0..4");
    a.parse().unwrap()..b.parse().unwrap()
}

/// The commands a seed's design runs, flattened.
fn commands(seed: u64) -> Vec<(String, serde_json::Value)> {
    fuzz::run(seed, 8).steps.into_iter().flat_map(|s| s.commands).collect()
}

/// Run the commands. For each failed command, check the design did not change.
/// Returns the session and a list of contract breaks.
fn replay(seed: u64, cmds: &[(String, serde_json::Value)]) -> (Session, Vec<String>) {
    let mut s = Session::default();
    let mut broken = Vec::new();
    for (i, (id, p)) in cmds.iter().enumerate() {
        let before: Document = (*s.doc).clone();
        if let Err(e) = s.execute(id, p)
            && *s.doc != before
        {
            broken.push(format!("seed {seed} cmd {i} `{id}` failed ({e}) but changed the design"));
        }
    }
    (s, broken)
}

/// The three clauses of the contract on each seed's design, from one fuzz run (the costly part):
/// a failed command leaves the design unchanged (clause 1); two replays of the same commands give
/// the same design and the same volume to the bit (clause 3); undo back to the start gives the
/// empty design and redo back to the end the final one (clause 2).
#[test]
fn fuzzed_designs_keep_the_transaction_contract() {
    let empty = (*Session::default().doc).clone();
    let mut broken = Vec::new();
    for seed in seeds() {
        let cmds = commands(seed);
        let (mut s, failed) = replay(seed, &cmds);
        broken.extend(failed);
        let (again, _) = replay(seed, &cmds);
        if *s.doc != *again.doc {
            broken.push(format!("seed {seed}: two runs of the same commands gave different designs"));
        }
        let (va, vb) = (volume(&s), volume(&again));
        if va.to_bits() != vb.to_bits() {
            broken.push(format!("seed {seed}: two runs gave volumes {va} and {vb}"));
        }
        let end: Document = (*s.doc).clone();
        let mut n = 0;
        while s.execute("edit.undo", &serde_json::Value::Null).is_ok() {
            n += 1;
            assert!(n < 10_000, "seed {seed}: undo never runs out");
        }
        if *s.doc != empty {
            broken.push(format!("seed {seed}: after {n} undos the design is not the empty design"));
        }
        for _ in 0..n {
            if let Err(e) = s.execute("edit.redo", &serde_json::Value::Null) {
                broken.push(format!("seed {seed}: redo failed: {e}"));
                break;
            }
        }
        if *s.doc != end {
            broken.push(format!("seed {seed}: after {n} redos the design is not the design before undo"));
        }
    }
    assert!(broken.is_empty(), "{} contract breaks:\n{}", broken.len(), broken.join("\n"));
}

fn volume(s: &Session) -> f64 {
    s.model.state().bodies.iter().filter_map(|b| solidcraft_kernel::measure(&b.body).ok()).map(|m| m.volume).sum()
}
