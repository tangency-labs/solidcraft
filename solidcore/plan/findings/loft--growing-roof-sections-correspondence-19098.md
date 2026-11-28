# Growing roof sections need a correspondence decision (#19098)

Status: investigated, not fixed. No production matching change is proposed.

## Reproduction

The report's 28-command script builds four roof sections and lofts them, then cuts
with the box `[-1, -80, -2] .. [2, 80, 28]`. The installed Linux x86_64 binaries
of both SolidCraft 0.44.0 and 0.44.1 refuse step 27 (0.44.1: two full
replays with the same failure; 0.44.0: one):

```text
SMLib boolean: operation: solidcore: SmApiBoolean: topology::brep::p6: no radial sector of the edge takes the new edge use
```

The replay command is `<installed-release>/bin/solidcraft-cli run
/home/ray/builds/codex-lanes/sub1-1118/repro-19098/r.json --results`.
Both full-script releases exit 1 at the reported box cut. The loft-only prefix
(`/tmp/19098-loft.json`) exits 0.

On 0.44.1, stopping after the first `inspect.measure` reports:

| Quantity | Observed |
| --- | ---: |
| Volume (mm³) | 30913.040324012625 |
| Faces | 54 |
| Edges | 156 |
| Vertices | 104 |
| Shell closed | true |

These are observations, not new expected values. The report's old-release volume
and face counts have not been adopted as a golden or independently derived here.
The reported interior fold has not been independently certified.

A software-rendered snapshot was captured with the 0.44.1 CLI using the script
through its first measurement, at 1280 × 720, isometric view, ultra quality,
without axes or grid. It shows the extra narrow strips near the front roof
corners; the image alone does not certify an interior fold. A second, close view
uses target `(1, 14, 0)`, yaw `2`, pitch `0.4`, distance `6` (angles in radians,
distance in mm), with the other rendering settings unchanged. It resolves the
narrow strips at the front corner. The captures are `/tmp/19098-before.png` and
`/tmp/19098-corner-before-outside.png`; both are session review artifacts, not
assets or geometric certificates.

## Source boundary

`crates/solidcore/src/sweep/loft_sections.rs`, `Grid::pair` and `Grid::alone`,
score a paired vertex by normalized distance and tangent difference, and a lone
vertex by its projection distance and tangent difference plus `UNPAIRED = 0.1`.
`correspond` chooses the least-cost walk, then creates cuts for lone vertices.
`match_loft_sections` carries those cuts to all earlier sections. Equal segment
counts do not constrain that walk to pair vertices one to one.

The input `LoftSection` holds the plane and a flat `Vec<Seg2>`. It has no source
sketch curve groups, fit-point ordinals within a source curve, or cross-section
correspondence constraints. The ops adapter passes only those values, and
SolidCraft's `loft_numbered_loops_by` also passes only the plane and flat segments.
`NumberedLoop::from` traces local profile segments, not a correspondence between
two sketches.

## Objective diagnostic

An independent Python reconstruction of the current cost and a full search over
all cyclic starts, using the report's exact decimal fit points, gives the
following on the original, uncut adjacent loops (47 pieces apiece):

| Sections (x) | Best walk cost | Pairs | Lone A | Lone B | Authored one-to-one cost |
| --- | ---: | ---: | ---: | ---: | ---: |
| 0 → 6 | 5.464699123438151 | 41 | 6 | 6 | 6.411718574967251 |
| 6 → 12 | 2.876055303474944 | 45 | 2 | 2 | 3.176292674831283 |
| 12 → 20 | 1.7658748786763598 | 47 | 0 | 0 | 1.7658748786763594 |

This is a reconstruction diagnostic, not a replacement for the Rust regression
or a geometric certificate. It interpolates each sketch's fit points with its
natural cubic, uses eight chords for nearest-point candidates as the matcher
does, and integrates area and moments by eight-point Gauss-Legendre. For these
polynomial cubics, the area integrand has degree at most five and each first
moment integrand degree at most eight, so the integration rule is exact in real
arithmetic (degree fifteen). No cut placement, carry propagation, skinning or
boolean is modeled by this diagnostic.

The first two differences are much larger than a rounding tie: preserving the
authored correspondence is strictly more expensive under the current objective.

A scratch check also weights a vertex by half the sum of its incident curve
lengths, divided by its loop perimeter. Weighting a pair by the mean of its two
vertex weights still leaves four lone vertices per side on the first section
pair; the maximum leaves six and the sum leaves eight. A lone vertex uses its
own weight in these experiments. These are approximate-length diagnostics, not
proposed objectives or certified constructions. They show that simple local
length weighting does not recover all 47 authored pairs here.

## Why the obvious changes are insufficient

Two existing regressions are relevant and pass on the current construction:

- `a_point_only_the_other_section_has_splits_a_straight_side`: two five-piece
  rectangles, one with its extra point on the left and the other on the right,
  need six matched pieces. Equal counts do not establish matching topology.
- `a_square_to_a_diamond_runs_its_corners_straight_up`: a square must run to the
  mid-side points of an eight-piece diamond. Requiring sharp corners to pair
  would turn this supported loft by 45 degrees.

Continuity labels alone do not supply that missing correspondence either. In
that five-piece rectangle example, the left loop has vertices
`[(0,0), (40,0), (40,20), (0,20), (0,10)]`, and the right loop has
`[(0,0), (40,0), (40,10), (40,20), (0,20)]`. Each has four corners and one
smooth joint. The unique cyclic mapping that preserves these labels pairs
left vertex 4 with right vertex 2, and hence left vertices 0, 1, 2, 3 with
right vertices 3, 4, 0, 1. Every pair obeys `(x,y) -> (40-x,20-y)`. At half
the loft height, linear interpolation therefore sends every paired vertex to
`(20,10)`: the section collapses. The intended six-piece geometric matching
keeps the rectangle throughout. This is an exact hand-derived counterexample
to using a unique smooth/corner pattern as an authored-compatibility contract,
not a sampled or tolerance-dependent test.

The review history recorded in `b7a034ce2` explicitly rejected forced corner
pairs: a square-to-diamond correspondence that runs straight up costs 4.40,
whereas forcing corner-to-corner correspondence costs 5.49 and turns the loft.
The commit says the pairing is least-cost at every size and corner pairs only
place the large-grid search band. These are historical review numbers, not
new measurements from this investigation.

Thus neither a count-based bypass nor forced corner anchors implements a general
solution while preserving the intent of #16943. Raising the unpaired cost or a
minimum cut spacing would introduce a heuristic and does not establish authored
correspondence. A cut-spacing adjustment also cannot prove that a skinned wall
is regular between the supplied sections.

A further scratch construction retains the least-cost walk's already paired
corner-to-corner anchors, and replaces the events inside an anchor interval by
ordinal pairs when both intervals have equal piece counts and equal ordered
smooth/corner patterns. On the original inputs it yields 47 events for each roof
pair, retains six for the opposite-extra-point rectangles, eight for the
square-to-diamond example, and nineteen for the existing hood pair. This is
only a correspondence diagnostic: neither the complete matcher, the resulting
skin nor the boolean has been evaluated with this policy. Passing those examples
does not prove that overriding the interior geometric pairs is the intended
construction. No production implementation is proposed.

A maintainer decision is needed on the correspondence contract: how to identify
compatible authored pieces, whether their source grouping should reach the
kernel, and how that interacts with geometric matching for genuinely different
sections. Alternatively, a redesigned geometry-only objective needs an agreed
set of correspondence invariants covering both this case and the existing
regressions. This investigation does not establish that no such objective can
exist; it does not have a justified one to propose.

## Historical boundary

The local object database contains `b7a034ce2` (#16943), which adds the entire
matching module. The parent kernel construction resamples to the largest input
segment count and chooses a cyclic seam, so it cannot introduce cuts into equal
47-piece sections. This is source evidence, not a CLI bisect.

The requested `v0.43.0` and `v0.44.1` refs are absent locally (`git log
v0.43.0..v0.44.1` fails with a bad revision), and GitHub API access fails to
connect. Lane instructions prohibit writing git metadata, so refs were not
fetched and `git bisect` was not run. The two installed 0.44 binaries confirm
the refusal is present in 0.44.0; they do not locate its introducing commit.

## Validation

All Cargo commands ran from the worktree root through
`buildq run solidcraft . -- cargo ...`.

| Command after the queue prefix | Result |
| --- | --- |
| `test --manifest-path solidcore/Cargo.toml -p solidcore --lib sweep::loft_sections::tests` | Passed: 35 tests, 0 failures, 5312 filtered out |
| `fmt --manifest-path solidcore/Cargo.toml --all` | Passed |
| `fmt --all` | Passed |
| `clippy --manifest-path solidcore/Cargo.toml -p solidcore --lib -- -D warnings` | Remote staging failed before compilation: `Disk quota exceeded`, exit 2 |

The focused temporary probe was
`sweep::loft_sections::tests::growing_roof_sections_keep_their_matching_pieces`,
placed next to the existing section-matching regressions. It derives the required
47 pieces as 3 lines + 2 right-skirt cubics + 40 crown cubics + 2 left-skirt
cubics. It has **no Rust test result**: an initial waiting client exited with
signal 143; later local-only retries remained queued and were cancelled. One
normal-dispatch replacement stopped at Cargo's argument parser (exit 101):
command-line `--config` does not support an inline table for `env.TMPDIR`.

Corrected requests used the separate scalar overrides
`--config 'env.TMPDIR.value="/home/ray/builds/lane-19098/tmp"'`
and `--config env.TMPDIR.force=true`, followed by:

- `test --manifest-path solidcore/Cargo.toml -p solidcore --lib
  sweep::loft_sections::tests::growing_roof_sections_keep_their_matching_pieces
  -- --exact --nocapture`
- `clippy --manifest-path solidcore/Cargo.toml -p solidcore --lib -- -D warnings`

Both corrected requests remained queued for more than 75 minutes. They were
cancelled before compilation when the investigation ended without a justified
production construction. Queue-cap refusals (exit 2) and cancelled requests are
not test or lint results. No more than two requests from this lane waited at
once. The temporary Rust probe was removed; its patch remains at
`/tmp/19098-regression.patch` for the active session. The existing Rust file is
byte-for-byte unchanged (`git diff --exit-code --
solidcore/crates/solidcore/src/sweep/loft_sections.rs`, exit 0).

`git diff --check` passed. No goldens, expected kernel-case values, production
tolerances or production matching behavior changed. Only this finding and
`COMMIT_MSG.txt` remain as unstaged files. There is no fixed behavior for CI to
validate yet; the full suite, wasm and full kernel rig remain CI work after an
actual fix, per the lane's standing instructions.
