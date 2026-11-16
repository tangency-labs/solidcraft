# Mutation testing

Mutation testing changes the code in small, behaviour-changing ways and asks whether the checks
notice. A mutant that every check lets through is behaviour nothing constrains: a missing
assertion, an unexercised branch, or an equivalent or unreachable mutation to triage away.

solidcore's correctness is proved directly, by closed forms, geometric checks and invariants
(AGENTS.md §1), so its own tests are the check that counts. `scripts/mutants.sh` also checks
the unit's cases against its golden; a golden only shows a result changed, and a mutant the
tests miss is a missing test whatever the golden says.

## Quick start

```sh
# One module, one representative shard, both tiers:
scripts/mutants.sh --module crates/solidcore/src/geom/vector2d.rs --unit geom.vector2d --shard 1/20

# Only the lines a PR changes, tier 1 (unit tests) only — cheap:
scripts/mutants.sh --in-diff-base origin/main --no-goldens

# Which mutants exist for a module (no build, no tests):
scripts/mutants.sh --module crates/solidcore/src/geom/vector2d.rs --list-only
```

Tier 1 is `cargo-mutants` with the package's unit tests as the oracle. Tier 2 patches
each unit-test survivor into the working tree, runs
`goldens --unit <id> --goldens` against the unit's golden, and reverts. The script patches the
tree, so the mutated files must be clean; it checks and refuses otherwise.

Output goes to `reports/mutants/<utc-stamp>/` (ignored by git): `mutants.out/`
(cargo-mutants' own records), `cargo-mutants.log`, `in-diff.patch` when used, and
`tier2.tsv` with one line per replayed survivor. `summary.txt` has the counts.

## Options

| option | meaning |
|---|---|
| `-m, --module FILE` | source file to mutate (globs allowed) |
| `-u, --unit ID` | unit whose goldens check the survivors (required for tier 2) |
| `-D, --in-diff-base REF` | mutate only lines changed from `REF` (`git diff REF...HEAD`) |
| `-p, --package NAME` | cargo package (default `solidcore`) |
| `-j, --jobs N` | mutants in parallel (default 2; the machine budget is shared) |
| `-s, --shard I/N` | one shard of the mutants, round-robin |
| `--timeout SECS` | per-mutant timeout (default 180) |
| `--max-replay N` | replay at most N survivors in tier 2 |
| `--re REGEX` | only mutants whose description matches `REGEX` (cargo-mutants `-F`; note that cargo-mutants does not apply this filter to "delete struct field" mutants, so a few extra ones may appear) |
| `--no-goldens` | tier 1 only |
| `--fail-on-both-tier` | exit 1 if any mutant passes both tiers |
| `--list-only` | print mutants, run nothing |
| `-o, --out DIR` | report directory |

## Cost

Measured on an 8-core machine, `nice -n 10`, `-j 2`, warm cargo target:

- tier 1: about **16 s per mutant** including `cargo test -p solidcore` (copy mode
  reuses the shared target directory; the baseline run adds one test pass).
- tier 2: about **1 min per survivor** (the kernel rebuild dominates; the unit run
  itself is milliseconds to seconds).
- examples: 15 mutants + 5 replays ≈ 4 min; 63 mutants + 17 replays ≈ 32 min.

The whole crate is far too large for one run (tens of thousands of mutants). Target a
module, a shard, or a PR diff. Shard a module across jobs when going wide, and keep
`-j` at 2 on a shared machine.

## Reading the results

Tier 1 writes `caught`, `missed`, `timeout` and `unviable` lists. Tier 2 classifies
each survivor as:

- **caught-by-goldens** — the goldens catch it. The unit tests are the weak layer; adding
  a unit test with an input that distinguishes the mutation keeps tier 1 meaningful and CI
  fast. Check the margin in the summary line: a mutation that changes 2 of 4,000 cases is
  barely covered.
- **both-tier-missed** — the interesting case. Triage it:
  1. add the missing unit assertion;
  2. extend the unit's record so the state it changes is in the golden (the runner work),
     and bless it with that justification;
  3. if it is equivalent or unreachable in the exercised configurations, record why
     and exclude it (`--exclude-re` or a `mutants.toml`) rather than leaving it on the
     list.
- **timeout** — the suite hangs instead of failing. Fix the test (a bounded loop) so a
  fault produces a failure, then re-run; hung CI jobs are worse than red ones.
- **unviable** — the mutation does not compile; ignore it.

## CI proposal

Not enabled here: a mutation job is a maintainer decision because it adds CI cost.
Two modes fit the existing workflow.

**Per pull request (advisory, cheap).** Mutate only the changed lines and run tier 1:

```sh
git fetch --no-tags origin main
scripts/mutants.sh --in-diff-base origin/main --no-goldens
```

Report the survivors in the job summary. This catches weak unit tests in new code
without gating the merge.

**Nightly (deeper).** One module and unit per job or matrix entry, tier 2 included, with
upstream's TestFiles from the same composite action as the goldens job:

```yaml
name: mutants-nightly
on:
  schedule: [{ cron: "17 3 * * *" }]
  workflow_dispatch:
    inputs:
      module: { description: source file, required: true, type: string }
      unit: { description: goldens unit, required: true, type: string }
jobs:
  mutants:
    runs-on: blacksmith-8vcpu-ubuntu-2404
    steps:
      - uses: actions/checkout@v4
      - run: rustup show active-toolchain || rustup toolchain install
      - uses: ./.github/actions/upstream
      - run: scripts/mutants.sh --module "${{ inputs.module }}" --unit "${{ inputs.unit }}" --shard 1/4
      - uses: actions/upload-artifact@v4
        if: always()
        with: { name: mutants-report, path: reports/mutants/ }
```

A matrix of `(module, unit, shard)` entries is maintainer-owned: start with the modules
whose golden records the fewest values and with units that have recent deferral
reductions. Track **both-tier-missed per module** over time, not the unit mutation score
alone — the score rewards assertions the goldens already provide.

## Pilot findings (2026-10-23)

Sampled shards on the code as of `67e9a5e9d`:

| module | mutants | caught by unit tests | unit survivors | caught by difftest | both tiers missed |
|---|---:|---:|---:|---:|---:|
| `geom::vector2d` (1/20 shard) | 15 | 10 | 5 | 5 | 0 |
| `topology::topology` (1/2 shard) | 63 | 37 | 17 (+5 unviable, 4 timeout) | 6 | 11 |

The 11 both-tier survivors in `topology.topology` are the change tracker's
bookkeeping — `TopologyChangeCallback::capture`'s arms for
`ChangeGeometry|PostEdit`, `SplitInBrep`, `TrimNoSplitInBrep`,
`ChangeGeometry|Split|Merge` and `Destruction`, its `depth >= 64` bound and
`operation == ChangeOwner` test, and `with_owners`'s `owners` field — plus
`remove_key`, `is_small_topology` and `TrackTopologyChanges::key`. The runner compares
marks, flags, user fields and the raw notification stream, but not the tracker's
accumulated add/rm/chg lists or owner map. The vector2d survivors were all caught by
difftest, but one threshold mutation by only 2 of 4,060 cases.
