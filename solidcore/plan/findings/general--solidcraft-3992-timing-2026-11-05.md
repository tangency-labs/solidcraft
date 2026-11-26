# general: SolidCraft recipe timing evidence and replay tooling (2026-11-05)

Unit: general benchmark tooling. PR: [#2841](https://github.com/tangency-labs/solidcore/pull/2841).
Inputs: the twenty fresh-model recipes from SolidCraft #3992, identified below.
This record compares completed evaluations; it does not certify kernel correctness.

## Historical evidence

The [numeric baseline](../../scripts/perf/baselines/solidcraft-3992-2026-11-05.json)
contains two separately collected experiments, each with three intended repetitions
of twenty recipes for two variants. Successful durations, unsuccessful durations,
censored repetitions, aggregate geometry, source revisions and executable hashes
are retained. Medians can be recomputed from `successful_seconds`.

The recipe archive is
[`3992/kernel-comparison-2-cases.zip` at `317e240e3b39b65e336841e6ea3a84e919304602`](https://raw.githubusercontent.com/kaltenbrunnermarkus/solidcraft/317e240e3b39b65e336841e6ea3a84e919304602/3992/kernel-comparison-2-cases.zip),
19,306 bytes, SHA-256
`e84fd2837e3b27596ec3f224be62ca2a3e65c4c50d751c84ea4f04a1a7f26a14`.
All twenty extracted JSON hashes match the measured recipe hashes. Downloading or
extracting the archive is a separate preparation step; the runner does neither.

| Experiment | Application | Kernel |
|---|---|---|
| Earlier application baseline | `8e2b0cd60d14b4152b12b8f1fdb2bcfdcd8c52f4` | truck v0.30.1 |
| Shipped dependency | `481687fff38057691faf84a6fd844e826c552a6c` | `8ddce901671f1cebd9214a31d21895d725e886c8` |
| Adapted application experiment | `481687fff38057691faf84a6fd844e826c552a6c` plus an isolated API adapter | `d93a19bd4e0ca3b141cc028ca73f9cc80be38efe` |

The earlier-application comparison changes the application as well as the kernel.
The latest-kernel experiment includes many kernel changes; it cannot isolate the
benefit of streaming QP. The adapter is not a proposed application compatibility fix
and is not included in this tooling PR. Seven of eight extension tests passed;
`extend_a_cylinder_round_its_axis` failed with `NotExact` because the adapter did not
consume returned parameter remapping. Error-mapping and hostile-input tests passed.
The failure remains unresolved. The twenty recipe command inventories contain
neither `surface.extend` nor `surface.trim`; results apply only to those recipes.

In the truck comparison, the Menger union recipe produced three incomplete results
(36.844, 36.798 and 36.915 seconds), and the 20-by-20 hole pattern timed out at
300.006 seconds; its remaining two repetitions were not attempted. These are not
fast successes. The shipped variant completed all sixty measured evaluations.
Both variants in the adapted-latest experiment completed all sixty evaluations.
For example, the box corner/rim fillet median changed from 1.895502 to 0.828490
seconds, and the all-edge box chamfer from 0.213752 to 0.091253 seconds. Other recipes
regressed: the cylinder cup median changed from 0.223753 to 0.254887 seconds.
Three repetitions on one setup do not establish portable regression thresholds.

These historical measurements used an ad-hoc predecessor, not the new runner.
They used release executables, niceness 10, fresh processes, one excluded warmup
per variant, rotated variant order, and a 300-second case limit. Wall time includes
process and wrapper startup. The compiler was rustc 1.99.1
(`58cf8f9c3`, 2026-10-13), with optimization level 3, thin LTO and 16 codegen units.
Hardware identity is not published. Exact thread count was not retained; the baseline
explicitly records it as unknown rather than inventing reproducibility metadata. No raw logs or environment identifiers are
published. Do not combine the shipped medians from the two separate collections.

Matching counts or recorded values do not prove B-rep identity. Repeatability
covers recorded metrics only. Loft cases have no reference-volume oracle; their
geometry differences remain unresolved. Reference-volume errors are measurements,
not automatically graded tolerances. No C++ parity gate or green unit claim follows
from this timing evidence.

## Reusing the runner

[`replay_recipes.py`](../../scripts/perf/replay_recipes.py) uses only Python's standard
library. Measurement requires Linux, GNU `/usr/bin/time`, and `nice`; analysis and
schema tests can run on Windows. Build every CLI separately with the project's
resource limits, then stop competing builds before timing. No compilation occurs
inside a measured interval. Use an isolated configuration directory and start from
[the example](../../scripts/perf/replay_recipes.example.json):

```sh
python3 scripts/perf/replay_recipes.py run replay-config.json measurements
python3 scripts/perf/replay_recipes.py analyze measurements
python3 scripts/perf/replay_recipes.py export measurements public-results.json
python3 -m unittest discover -s scripts/perf -p test_replay_recipes.py -v
```

Binary and recipe paths resolve relative to the private configuration file. The
output directory must not already exist. Choose deliberately public, neutral IDs
and supply exact 40-character application/kernel revisions where known. Optional
adapter and recipe-archive SHA-256 values identify separately prepared inputs.
The runner hashes executables, recipe JSON and its own source, then rechecks the
input hashes after measurement. An input change makes all medians ineligible.
Configuration requires an explicit thread count; the optional `max_load` is a
preflight limit checked before each sample and aborts rather than silently waiting.
It does not prove absence of competing workloads.

Each variant gets separate XDG configuration/cache/data directories and an explicit
`RAYON_NUM_THREADS`; inherited `SOLIDCRAFT_` and `SOLVECRAFT_` overrides are removed.
Other inherited environment settings are not recorded: use a controlled environment
when comparing builds. One warmup evaluation per variant is excluded. Each measured
sample uses a fresh process, with variant order rotated each round. The parent
blocks while waiting for process exit; a separate watchdog enforces the timeout,
avoiding the polling delay of a timed subprocess wait on short recipes.

`manifest.json` records the measurement state and declared provenance;
`runs.jsonl` retains completed samples; `summary.json` includes failures and
unattempted repetitions separately. A successful sample requires exit zero, valid
resource metrics, finite geometry, at least one closed body, no timeline errors,
no suppressed or rolled-back features, and no skipped open bodies. Warnings remain
visible and are not automatically excluded. Empty-body recipes are outside this
solid-recipe benchmark contract. Timing eligibility is separate from correctness.

A timeout terminates only the runner's process group and censors that variant's
remaining repetitions for the same case. Interrupted or aborted experiments retain
samples but do not produce eligible medians. Ctrl-C and SIGTERM use the same
interruption path, including during warmup. An evaluator that leaves descendants
in its process group after its leader exits is cleaned up and recorded as
`descendants_remaining`, not a successful sample. This is process containment, not
a sandbox: executables must be trusted; processes that deliberately create a new
session and uncatchable termination are outside this cleanup contract.

Aborts record and print only an allowlisted `abort_reason`: `load_exceeded`,
`invalid_sample`, or `io_error`. Raw exception text is never exported.
Exit status zero means orchestration
completed: individual failed samples can still exist, so consumers must inspect the
summary. Exit status two means configuration, warmup, interruption, load, input
mutation or another orchestration failure prevented a complete experiment.

Raw output and evaluator JSON remain under `measurements/private`; those files may
contain sensitive data and must not be published. The explicit export command
copies only schema-validated labels, revisions, hashes, numeric values and booleans.
It does not read raw logs or copy arbitrary strings from evaluator output. Review
public IDs before exporting. Schema allowlisting cannot determine whether a human
has intentionally put a personal identifier into an otherwise valid label.

This work is separate from the closed, unmerged direct-kernel benchmark proposal
[#1272](https://github.com/tangency-labs/solidcore/pull/1272), existing CPU sampling
tools under `scripts/perf`, and the small-profile mitre investigation in #2840.
It adds no kernel changes, adapter fixes, profiler changes, or CI timing threshold.
