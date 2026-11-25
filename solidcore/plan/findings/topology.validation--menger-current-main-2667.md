# Menger level-2 validation on current main

Issue #2667; investigation PR #2925. Kernel base `513e6fcfc`, default kernel
options, optimized `difftest` profile. This is a reproduction, not an optimization.

## Input and protocol

The source is `kernel-comparison-2/cases/menger-level2-cuts.json` in the
[archived comparison inputs](https://github.com/kaltenbrunnermarkus/solidcraft/blob/317e240e3b39b65e336841e6ea3a84e919304602/3992/kernel-comparison-2-cases.zip).
Its SHA-256 is `5f1547971e2f487323aa5aabd47e0dc9c2ced9b52871e32d3c691e0ae0448bd9`.
All 27 centers, dimensions and their order in `examples/menger_validation.rs`
were compared exactly with that verified recipe. This is a 45-unit cube with
nine X cuts, nine Y cuts and nine Z cuts, each group beginning with its larger
central bar. The analytic final volume is `45^3 * (20/27)^2 = 50000`.

Run `cargo run -p solidcore --profile difftest --example menger_validation`.
Each row separates the boolean from a Level2 check and a standalone coincidence
check. Each diagnostic receives an independent `brep_copy` and default kernel;
copying is outside the timed region. The identity snapshot supplies the copied
body's geometry-owner keys. The standalone coincidence call uses the same
arguments as `check_brep`: body tolerance, first pair, report level zero.

This is not the original author's unavailable direct-library harness, and is not
a SolidCraft end-to-end timing. In particular, independent copies have different
cache histories from repeatedly checking the live result. The standalone
coincidence duration is not a measured nested share of the Level2 call and must
not be subtracted from it as an exact phase breakdown.

## Three retrieved runs

All 27 results pass Level2 and have zero coincident pairs. The final body has
414 faces, 984 edges and 488 vertices; integrated volume is
`49999.99999999997`, within `1e-9` relative of the analytic volume.

| Observation across 27 cuts | Median of three runs |
|---|---:|
| Boolean duration | 0.476993298 s |
| Level2 validation duration | 3.118145969 s |
| Standalone coincidence duration | 1.281565248 s |
| Boolean face pairs visited | 20,244 |
| Boolean inner loops classified | 460 |

The [numeric record](menger-validation-2667.json)
contains every row from all three runs. Geometry, validity, coincident-pair
counts and recorded boolean work counts agree across all runs. Each final volume
is `49999.99999999997`. These are observations with no warmup or timing threshold. They show that validation remains worth investigating on this
reproduction, not that any particular algorithm is the cause.

## Validation and next step

The example runs successfully in three fresh Linux processes; targeted clippy
with warnings denied also passes. The example was formatted with rustfmt and
`git diff --check` passes. Totals by run (seconds):

| Run | Boolean | Level2 validation | Standalone coincidence |
|---|---:|---:|---:|
| 1 | 0.475971991 | 3.118145969 | 1.257896030 |
| 2 | 0.476993298 | 3.092468362 | 1.281565248 |
| 3 | 0.480489875 | 3.164252900 | 1.284540602 |

No kernel behavior, tolerance, unit status or known-mismatch classification is
changed. Existing counters measure boolean face pairs and loop classifications;
coincidence candidate/narrow-phase counts are explicitly null, not zero. The next
step is to coordinate narrowly scoped coincidence instrumentation before choosing any optimization. The previous curve-cache and
UV-hull changes are already present in this base; this evidence does not imply
that they failed or that their earlier work should be duplicated.
