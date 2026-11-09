# Mutation testing

SolidCraft will move from truck to solidcore (#1597). The tests that have to catch regressions
across that switch are the ones that don't depend on the kernel: the reference tests
([docs/oracle.md](oracle.md)) and the engine's command tests. The reference for what the kernel
builds is the kernel-cases ladder (`tests/kernel-cases/ladder/`,
[docs/kernel-ladder.md](kernel-ladder.md)). Most of the kernel's own unit tests call
truck-specific code and go away with it.

Mutation testing checks those suites. [cargo-mutants](https://mutants.rs) makes small changes to
the source, one at a time (`*` becomes `/`, a condition is negated, a function returns a default
value), rebuilds, and runs the tests. A mutant the tests don't catch is a change in behaviour
nobody would notice, and solidcore could get the same thing wrong without anyone noticing.

## What judges a mutant

`.cargo/mutants.toml` judges mutants by the black-box suites only:

- `oracle_suite` (`apps/solidcraft-cli/tests/oracle_cases.rs`): every case in `tests/oracle` with
  expected values and no `known_failure` must pass, as `cargo xtask oracle` requires. It is
  `#[ignore]`d so CI doesn't replay the oracle twice.

The kernel's unit tests are left out on purpose. A mutant that only they catch marks a behaviour
the black-box suites would miss once the kernel is swapped.

## Running it

```
cargo install cargo-mutants --locked
CARGO_TARGET_DIR=$PWD/target cargo mutants --in-place \
  -f crates/kernel/src/measure.rs \
  --test-package solidcraft-cli --test-package solidcraft-engine \
  --baseline skip --timeout 900 --build-timeout 900
```

- Use a worktree you don't otherwise touch: `--in-place` writes each mutant into the source tree
  and restores it afterwards.
- `--shard 0/200 --sharding round-robin` with `-d crates/kernel` takes an even sample (every 200th
  mutant) of the whole crate.
- The `mutants` profile in `Cargo.toml` is the `ci` profile with incremental builds. It cuts the
  rebuild after a kernel change from about 3 minutes to about 15 seconds.
- A mutant costs 1.5–5 minutes on a 12-core Mac, mostly the tests. The kernel has about 40,000
  mutants, so run a sample or one file at a time, never the whole crate. This is a tool for
  finding gaps, not a CI gate.

Two problems with cargo-mutants 27.1 are behind the flags above:

- The baseline (the unmutated run) ignores `--test-package` and tests the mutated crate, which
  fails. Skip it with `--baseline skip`, set `--timeout` yourself, and check the unmutated tree
  passes first.
- `--re` and `--exclude-re` don't filter "delete field" mutants. Saved diffs in
  `mutants.out/diff/` name the mutation, not the file, in their `+++` line: apply one with
  `patch <file> < <diff>`.

## Pilot results (October 2026, `cb857f1f`)

| Sample | Mutants that build | Caught by the black-box suites |
|---|---:|---:|
| `crates/kernel/src/measure.rs`, every mutant | 85 | 85% |
| Whole kernel, every 200th mutant | 173 | 15% (95% interval 10–21%) |

Out of 49 of the kernel sample's survivors, the kernel's own unit tests caught 21 (43%) and
nothing caught 28 (57%). About 11% of survivors swap `>` and `>=` on floating-point values that
are never exactly equal, so they don't change behaviour.

The black-box suites are weakest on tessellation and the display mesh (0 of 19 mutants caught),
offset, shell, emboss and lip (0 of 14), blends (15%) and STEP/IGES exchange (17%). For a
survivor that a kernel unit test catches, that test's design is the starting point for a new
oracle case or engine test.
