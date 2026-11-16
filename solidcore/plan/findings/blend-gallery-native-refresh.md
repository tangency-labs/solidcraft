<!-- SPDX-License-Identifier: Apache-2.0 -->

# Blend gallery: native refresh, 2026-11-06

## Scope and result

Fourteen pictures still showed a state older than their current corpus entries. Their bytes
matched the old SMLib gallery even though the case's state had changed. Replace those pictures,
not the 260 other pictures, and retain the generated gallery and its existing status data.
Only the pictures and `gallery_note` fields change; no expected value, operand, call, tolerance,
fix set or known-failure entry changes.

The generated fillet inventory is **235 pass, 20 expected refusals, 19 known failing** (274).
That inventory is not a claim that every workflow or every runner passes.

Fresh native operation replay on the revision below, in four isolated processes:

```text
274 cases: 239 matching results, 0 fail, 16 runner-unsupported, 0 unported, 19 known-failing
```

Matching results include refusals. Unsupported cases are not counted as successful operations.
The known-failure list stays byte-for-byte unchanged:
`d5115c71928a30b5c59f4e7cc786dca2c32b293749df873ef8ea3fc4c55f027c` (SHA-256).

## Measured sources

Numerical replays and capture use the measured revision below. The integration base is
`03b29afea43db1f7cfc0a1941ec93708036946fd`; its corpus grading data are unchanged, and its
new elliptical-chamfer picture is preserved.

| Input | Revision |
|---|---|
| solidcore and its corpus | `85bf36811a32670e20c0e147c326129213d42cca` |
| SolidCraft base | `871014f955a76fd308ce156e68c93e6372906005` |
| Published consumer adapter | `2e8847512baf43c91ab4e2c407750d6f896b6053` in [the kernel-pin update](https://github.com/tangency-labs/solidcraft/pull/12413) |
| Complete local consumer diff, including Cargo.lock | SHA-256 `3b304405a8da353efbd7e7badaca077a2c9e5b49466239387e85f0fcda44fe99` |
| Measured CLI | SHA-256 `c209307a0383e3a70cf83be6a05dbd4703cdeccfc61304c9231e0cb8b948fae9` |
| Historical SMLib alpha measurement, not rerun here | `a18bbe523b9c7819f1a9da8c40a54c15f9936125` |
| SMLib fork main/release checked, not measured | `8eac63c439b2670ecffcc2d7beaf2242849463bf`, `prebuilt-8eac63c439b2670ecffcc2d7beaf2242849463bf` |
| Legacy reading reference, not executed | `scripts/upstream-pin.env`, `3fd5b9548d19aa8c12c29bc9637a96d42a06f8a5` |

SolidCraft main's older kernel API does not compile directly against this solidcore revision.
The temporary consumer applies the published adapter's four source files (`core.rs`,
`core/offset.rs`, `core/trim.rs`, `lib.rs`) to the SolidCraft base, then removes assignments to
these additional retired flags, whose fields no longer exist:

```text
precise_properties_split_at_surface_knots
fillet_setback_from_start_vertex
fillet_point_bigger_step_retry
offset_extends_cones_as_themselves
ssi_no_reverse_of_empty_trace
precise_properties_trim_base_v
point_set_centroid_all_blocks
precise_properties_span_end_from_below
offset_extends_ball_patches_as_themselves
```

No supported flag choice from that adapter is changed. In particular its two held-back flags
stay held back. Both `solidcore` and `solidcore-record` are Cargo path overrides to the measured
checkout. The consumer edits are not part of this gallery change. The corpus keeps its own
recorded fix set; the consumer and corpus are not interchangeable parity runs.

The CLI was built with Rust 1.98.0, `--release --no-default-features`, jobs 2, incremental off,
release opt-level 1, debug info off and LTO off. No C++ backend is built or run.

## Application cross-check, kept separate

Run the unmodified application recipes and expectations through this CLI, one process per case,
with `--json --validation=each_feature`. Of the 274 corpus identities, 265 have application
recipes: **245 pass (224 measured outputs, 21 accepted refusals), 9 fail, 11 unsupported**.
The other nine exist only in the kernel corpus; no application result is invented for them.

The native corpus still lists `report-peg-front-and-concave-chamfers-together-d2` as failing,
although it passed in the historical alpha gallery. Its retained reason is the corner standing
`7.2e-4` out of the front face, beyond the `4e-5` box. The current native replay also retains its
listed volume mismatch. The complete consumer recipe passes. Different inputs/construction
history and fix sets prevent calling that a same-input regression or removing the native entry.

Two cross-check differences appeared in the audited cases:

- `2893-chamfer-distance-and-angle-past-the-wall-refused`: the application refuses, but reports
  generic invalid input instead of the expected angle message. Its picture and note say so.
- `2893-chamfer-two-distances-elliptical-rim`: the native operation on the stored operand passes,
  but the full application recipe refuses at distance `0.50268`. The independent cross-check is a
  native operation result, not a successful full-workflow claim. The new image on main is kept.

The application accepts the partial-chain case's allowed refusal while that case remains on the
native known-failure list. Keep its reason and show only the input, not an invented round.

Application nonpasses (unchanged expectations; a check name is not a diagnosis):

| Case | Status | Message or failed checks |
|---|---|---|
| `1080-fillet-equal-slope-basin-1-mm-rim-loop-4` | fail | checks: continuity_order |
| `1080-fillet-sloped-basin-1-mm-rim-r4-2-edges` | fail | checks: continuity_order |
| `1080-fillet-sloped-basin-1-mm-rim-r4-4-edges` | fail | checks: continuity_order |
| `2893-chamfer-distance-and-angle-past-the-wall-refused` | fail | refused, but not because "angle": step 2 (solid.chamfer): SMLib chamfer: operation: SMLib rejected the input (SM_ERR_INVALID_INPUT) |
| `2893-chamfer-two-distances-elliptical-rim` | unsupported | step 3 (solid.chamfer): not supported yet: the chamfer is larger than the faces beside its edges allow, at the edge at (7.516, -4.479, 10.000); the largest distance that fits is 0.50268 mm (tangent propagation blended the picked edges' tangent chain with them; with it off, tangent_chain: false, only the picked edges are blended) |
| `4782-fillet-bar-edge-wider-than-both-faces` | fail | checks: continuity_order |
| `9-fillet-corner-curvature-with-reduce-fit` | unsupported | solid.fillet builds `corner: g2` with `fit: exact` only |
| `9-fillet-corner-curvature-with-taper-fit` | unsupported | solid.fillet builds `corner: g2` with `fit: exact` only |
| `cov-fillet-g2-five-edge-apex-corner` | fail | step 12 (solid.fillet): invalid input: the setback must be more than 3.5056 mm at the corner (-0.000, 0.000, 30.000): the rounds' contact lines meet 3.5056 mm from it along its edges, and the corner's patch starts past that |
| `cov-fillet-g2-four-edge-apex-corner` | fail | step 11 (solid.fillet): invalid input: the setback must be more than 3.9027 mm at the corner (0.000, 0.000, 30.000): the rounds' contact lines meet 3.9027 mm from it along its edges, and the corner's patch starts past that |
| `cov-fillet-g2-mixed-corner` | unsupported | step 3 (solid.fillet): invalid input: not supported yet: setting back the corner at (15.000, 0.000, 10.000) with the SMLib kernel (a G2 corner is set back): SMLib sets back a corner only where every edge meeting it is rounded and they are all convex or all concave |
| `cov-fillet-g2-oblique-three-edge-corner` | unsupported | step 8 (solid.fillet): invalid input: not supported yet: a curvature-continuous fillet on solidcore over edges whose faces meet at different angles: every edge takes the equal-area thumbweight of the first one's angle, so the others take off the wrong area (https://github.com/tangency-labs/solidcore/issues/2250#issuecomment-6563419897) |
| `cov-fillet-g2-pocket-corner` | unsupported | step 3 (solid.fillet): invalid input: not supported yet: filling this loop with one surface meeting its neighbours: the closest fit stays 4.964211487379355e-6 off the loop (tolerance 7.280109889280518e-5), turns 0.0107° from its neighbours (tolerance 0.0500°) and misses their curvature by 4.0237928773736355e-3 (tolerance 2.5e-3) |
| `cov-junction-boss-tilt-20-root-round` | unsupported | step 3 (solid.fillet): not supported yet: the fillet is larger than the faces beside its edges allow, at the edge at (-0.637, 5.970, 0.000); the largest radius that fits is 0.262 mm (tangent propagation blended the picked edges' tangent chain with them; with it off, tangent_chain: false, only the picked edges are blended) |
| `cov-junction-tee-equal-round` | fail | step 3 (solid.fillet): SMLib fillet: operation: SMLib failed (status 1001) (tangent propagation blended the picked edges' tangent chain with them; with it off, tangent_chain: false, only the picked edges are blended) |
| `cov-junction-y-junction-120-round` | unsupported | step 4 (solid.fillet): not supported yet: the fillet is larger than the faces beside its edges allow (tangent propagation blended the picked edges' tangent chain with them; with it off, tangent_chain: false, only the picked edges are blended) |
| `report-fillet-curvature-three-edges-at-bar-corner` | unsupported | step 13 (solid.fillet): invalid input: not supported yet: filling this loop with one surface meeting its neighbours: the closest fit stays 4.964211486935266e-6 off the loop (tolerance 5.296085413780557e-5), turns 0.0107° from its neighbours (tolerance 0.0500°) and misses their curvature by 4.023792878055378e-3 (tolerance 2.5e-3) |
| `report-fillet-curvature-three-edges-at-bar-corner-solver-residues` | unsupported | step 13 (solid.fillet): invalid input: not supported yet: filling this loop with one surface meeting its neighbours: the closest fit stays 4.964211487379355e-6 off the loop (tolerance 5.4014485451968786e-5), turns 0.0107° from its neighbours (tolerance 0.0500°) and misses their curvature by 4.02379288186469e-3 (tolerance 2.5e-3) |
| `report-fillet-curvature-three-edges-at-bar-corner-solver-residues-outside-setback` | unsupported | step 13 (solid.fillet): invalid input: not supported yet: filling this loop with one surface meeting its neighbours: the closest fit stays 4.964211487379355e-6 off the loop (tolerance 5.4014485451968786e-5), turns 0.0107° from its neighbours (tolerance 0.0500°) and misses their curvature by 4.02379288186469e-3 (tolerance 2.5e-3) |
| `cov-fillet-g2-corner-beside-a-cylinder` | fail | step 6 (solid.fillet): SMLib fillet with open setback corners: operation: SMLib's result is not a closed solid |

Corpus-only cases without an application recipe:

- `11991-round-cylinder-edge-ending-on-a-round-tangent-line`
- `12034-round-loft-square-to-circle-circular-rim`
- `12342-round-wall-and-ledge-edges-r2-5`
- `12342-round-wall-and-ledge-edges-r5-refused`
- `12342-round-wall-edge-r5-past-the-wall-refused`
- `2903-edited-rim-chamfer`
- `2903-edited-rim-round`
- `2903-original-rim-chamfer`
- `2903-original-rim-round`

## Pictures and checks

Fourteen pictures are actual CPU snapshots from the compatible consumer. Eleven show completed
recipes; three show input before the stopped operation, with all later commands left out.
An independent cross-check capture shows the native elliptical-chamfer result through
`solidcore-diag`: 2,406 triangles, zero refused faces. Main refreshed that image in
`151c6f2e9` during this work, so this change keeps the maintainer's image. The cross-check
capture is evidence only, not another replacement.

All 14 replacement pictures were inspected at full size. The 11 completed application capture models match
their case run's body, face, edge and vertex counts exactly and volume/area within `1e-9`
relative. The native capture's SMB round trip preserves counts and volume within `1e-9` relative.
The pictures illustrate the result; they do not prove continuity from shading. The CPU renderer
provides no surface-analysis overlays.

The main view follows the case's issue: the peg's front tangent junction, the pipe/bar end
section, the knife-edge round's end, the boss foot or the stopped corner. Related geometries
share a direction; R7 has a wider view than R2. Whole-part insets use the saved model's exact
reported box, not seam vertices (a circular edge can have just one seam vertex).

Application capture: run the case's commands with `$CASE` and `$OUT` expanded, save with
`solidcraft-cli run SCRIPT --save MODEL.sc --json-out MODEL.json`, then:

```sh
SOLIDCRAFT_GPU=cpu solidcraft-cli snapshot --in MODEL.sc --out detail.png   --width 1080 --height 680 --camera CAMERA_JSON --quality fine --no-grid --no-axes
```

The context uses the same yaw/pitch, target at the saved box's centre, distance 1.5 times its
diagonal, size 360x240. For a stopped case, SCRIPT ends before the last fillet/chamfer.
The native ellipse is captured immediately before `measure(&result)` in
`crates/solidcore-ops/tests/kernel_cases.rs`, writing `result[0].body.to_smb(false)` only for that
case; this reporting-only hook is removed afterwards. Run the unchanged case to grade it, then:

```sh
solidcore-diag ellipse.smb --binary --view -35,-35 --size 2400x1600 --edges-above 1000000
```

The independent native crop is (820, 0, 2220, 882); it is not shipped here. Composition only adds a whole-part inset and caption;
no geometry is drawn or retouched. Application insets are 270x180 at (792, 12); the native
ellipse's inset is at (16, 480). Canvas 1080x760, caption starts at y=680, JPEG quality 92,
no chroma subsampling. Positions/distances below are millimetres, yaw/pitch radians.

```json
[
  {
    "case": "2893-chamfer-distance-and-angle-past-the-wall-refused",
    "source": "SolidCraft recipe (input only)",
    "camera": {
      "target": [
        0,
        0,
        20
      ],
      "yaw": -0.6108652381980153,
      "pitch": 0.5235987755982988,
      "distance": 20.462962962962965,
      "fov": 0
    },
    "sha256": "03af58ed7ae3e9ec7ef1afb203be9a373abd81b618cae7b96cf61e3aed8dbf4e"
  },
  {
    "case": "5158-fillet-round-just-past-a-10mm-face",
    "source": "SolidCraft completed recipe",
    "camera": {
      "target": [
        36,
        5,
        13
      ],
      "yaw": -0.9599310885968813,
      "pitch": 0.5235987755982988,
      "distance": 22,
      "fov": 0
    },
    "sha256": "648f1151ea1beb66dfbccedacd11dc276b7affed0592fee84e37c31669a605fc"
  },
  {
    "case": "851-fillet-arc-then-top-front",
    "source": "SolidCraft completed recipe",
    "camera": {
      "target": [
        40,
        -20,
        8
      ],
      "yaw": -0.6108652381980153,
      "pitch": 0.6108652381980153,
      "distance": 14.953703703703704,
      "fov": 0
    },
    "sha256": "648117bad578cd9945f55723036684f0797e1cfa5ba0c7acca10e4878e698a5f"
  },
  {
    "case": "851-fillet-top-front-then-upright",
    "source": "SolidCraft completed recipe",
    "camera": {
      "target": [
        40,
        -20,
        8
      ],
      "yaw": -0.6108652381980153,
      "pitch": 0.6108652381980153,
      "distance": 14.953703703703704,
      "fov": 0
    },
    "sha256": "b4234550a59b6271950c649b48d342806100430fb5c9f41c51289c4429d6645e"
  },
  {
    "case": "cov-fillet-section-curvature-pipe-on-plate-r2",
    "source": "SolidCraft completed recipe",
    "camera": {
      "target": [
        56,
        28,
        12
      ],
      "yaw": -2.007128639793479,
      "pitch": 0.4363323129985824,
      "distance": 22,
      "fov": 0
    },
    "sha256": "76ce875fc2a25fdec8b4af8f154a1003704ae908ae734013e685a4c31a1c87b4"
  },
  {
    "case": "cov-fillet-turn-swept-boss-base-r1",
    "source": "SolidCraft completed recipe",
    "camera": {
      "target": [
        15,
        10,
        10
      ],
      "yaw": 0.6108652381980153,
      "pitch": 0.6981317007977318,
      "distance": 11.018518518518519,
      "fov": 0
    },
    "sha256": "751a7059c09c2133754d950e68f814db7d58808f2772f040a66011fd1623e567"
  },
  {
    "case": "cov-fillet-turn-swept-pocket-sharp-corner-refused-r1",
    "source": "SolidCraft recipe (input only)",
    "camera": {
      "target": [
        10,
        20,
        5
      ],
      "yaw": -0.6108652381980153,
      "pitch": 0.9599310885968813,
      "distance": 13.37962962962963,
      "fov": 0
    },
    "sha256": "caf30ebc402902c223c0ac440defdc4cadace294d2ad15ff7a4d866e7fc02015"
  },
  {
    "case": "every-block-cylinder-tangent-concave-arc-r2-partial-chain",
    "source": "SolidCraft recipe (input only)",
    "camera": {
      "target": [
        0,
        -10,
        15
      ],
      "yaw": 0.3490658503988659,
      "pitch": 0.6108652381980153,
      "distance": 13.37962962962963,
      "fov": 0
    },
    "sha256": "67b1403e44abfbb4886c5d64133cb0ce7abfd6eedbf04c4844bb371d3736d8d7"
  },
  {
    "case": "report-fillet-concave-arc-and-front-top-edge-together-r5-solver-residues",
    "source": "SolidCraft completed recipe",
    "camera": {
      "target": [
        37.5,
        2.2,
        10
      ],
      "yaw": -0.3490658503988659,
      "pitch": 0.6108652381980153,
      "distance": 15.740740740740742,
      "fov": 0
    },
    "sha256": "0aaf466031de05d212138adcceb846769fe5e04e5b01914d76b9ed32f4b4a148"
  },
  {
    "case": "report-fillet-concave-arc-and-front-top-edge-together-r7-solver-residues",
    "source": "SolidCraft completed recipe",
    "camera": {
      "target": [
        36.5,
        4,
        11
      ],
      "yaw": -0.3490658503988659,
      "pitch": 0.6108652381980153,
      "distance": 22,
      "fov": 0
    },
    "sha256": "7960254baf291207b1c1a811ca1d761fd675146adb75928a3c3cf1ba5a7f2d64"
  },
  {
    "case": "report-fillet-concave-arc-and-front-top-edge-together-solver-residues",
    "source": "SolidCraft completed recipe",
    "camera": {
      "target": [
        37.5,
        2.2,
        10
      ],
      "yaw": -0.3490658503988659,
      "pitch": 0.6108652381980153,
      "distance": 12.592592592592593,
      "fov": 0
    },
    "sha256": "973d78a9dcd980ad97bbaae46000c4addf97810a1055338bf446f4aed68ff52e"
  },
  {
    "case": "report-peg-front-and-concave-rounds-together-r2",
    "source": "SolidCraft completed recipe",
    "camera": {
      "target": [
        36,
        2,
        10
      ],
      "yaw": -0.3490658503988659,
      "pitch": 0.6108652381980153,
      "distance": 12.592592592592593,
      "fov": 0
    },
    "sha256": "15ff39e6cc7bd6a3b75cd1f39b625d10b57fa6890a2f12f02afe90bbef134619"
  },
  {
    "case": "report-peg-front-and-concave-rounds-together-r5",
    "source": "SolidCraft completed recipe",
    "camera": {
      "target": [
        36,
        2,
        10
      ],
      "yaw": -0.3490658503988659,
      "pitch": 0.6108652381980153,
      "distance": 15.740740740740742,
      "fov": 0
    },
    "sha256": "9e5926fb0bf558e30cb229ad361ff303cbd4cd1d172d93b8328dde353f47215a"
  },
  {
    "case": "report-peg-step-front-and-concave-rounds-together-r2",
    "source": "SolidCraft completed recipe",
    "camera": {
      "target": [
        36,
        2,
        10
      ],
      "yaw": -0.3490658503988659,
      "pitch": 0.6108652381980153,
      "distance": 12.592592592592593,
      "fov": 0
    },
    "sha256": "e1af978dea5ec2b49431ce1f626a4bb189b28058a67da1b191da7355767d5ef4"
  }
]
```

Local checks: workspace fmt and clippy (`-D warnings`), wasm32 kernel check, all 74 CI-script
unit tests, generated-gallery identities/counts, unchanged grading fields and known-failure
hash, picture pairing/decode and whitespace checks pass. No full kernel-suite pass is claimed:
the all-at-once build exceeded temporary storage. Bounded runs passed 216 integration
executables, including a clean rebuild of the incomplete storage-failure artifact; two unchanged
targets (`issue_2406_end_rail_rollover`, `wr698_gn_round_is_its_definition`) exceeded a 300-second
local process limit. Remaining suite coverage and the hosted CI gate are still required.
