# Action matrix

The action matrix tries every ordinary action on every face, edge, vertex and body of the final
bodies of a corpus of real designs, through the same commands and parameters the desktop sends,
and records what works. [`tests/action_matrix/baseline.json`](../tests/action_matrix/baseline.json)
is a ratchet: an action that passed there and fails now is a regression and fails the test.
The test is [`apps/solidcraft-cli/tests/action_matrix.rs`](../apps/solidcraft-cli/tests/action_matrix.rs).

## Running it

- **Every CI run** (pull requests too) checks a fixed sample, one action in 40 by a stable hash of
  its name, on every design except the slowest to build (the `SLOW` list, and any design that
  takes more than 8 s to load in costs.json; `action_matrix_sample`, a few minutes).
- **Nightly** (and by hand, Run workflow), the "Action matrix" jobs run the whole matrix in 16
  shards (`action_matrix_shard` with `ACTION_MATRIX_SHARD=k/16`) of about equal cost, cut by
  [`tests/action_matrix/costs.json`](../tests/action_matrix/costs.json): what each design took on a
  runner. A design that takes more than ten minutes is split into slices (actions by a hash of their name, each
  slice loading the design again), and the pieces go largest first to the shard with the least
  work. A design not in costs.json yet counts as two minutes. Each shard uploads its outcomes and
  timings (artifact `action-matrix-k`); with all sixteen in one folder,
  `cargo xtask action-matrix --from DIR --bless` writes the baseline and costs.json from them
  without running anything. `ACTION_MATRIX_SHARD=k/n ACTION_MATRIX_SHARD_LIST=1` prints the plan.
- **Determinism** (nightly, "Action matrix determinism"): `action_matrix_determinism` runs one
  action in 8 of every design that takes at most five minutes twice on one runner and lists every action whose outcome changed; it fails when a
  family of pieces passes a different number of times.
- **Locally**: `cargo xtask action-matrix` runs it whole (a few hours on one machine now; the nightly shards' artifacts and `--from` are quicker). Add `--bless` to
  rewrite the baseline after a fix (newly passing actions are listed when you don't), and
  `--results FILE` to keep every outcome with its message and the command that was sent.
  `ACTION_MATRIX_FILTER=oracle/23` runs only the designs whose name contains it;
  `ACTION_MATRIX_JOBS` sets how many designs run at once.
  For a quick local look, `ACTION_MATRIX_ACTIONS=edge.fillet,edge.chamfer` tries only those actions (never
  bless such a run) and `ACTION_MATRIX_VALIDITY=1` adds each passing action's body check (`Body::validity`)
  to the results.
  `ACTION_MATRIX_UNFOLD=1` also unfolds and refolds every sheet metal body after each action that
  passed, recorded as `<action>+unfold`. To recheck one design on its own:
  `ACTION_MATRIX_DESIGN=oracle/69-sm-tray ACTION_MATRIX_OUT=out.jsonl ACTION_MATRIX_ACTIONS=face.hole
  cargo test --profile ci -p solidcraft-cli --test action_matrix action_matrix_worker -- --exact --ignored`
  (from `apps/solidcraft-cli`).

Each design runs in its own process, so an action that hangs (90 s without a result) or aborts is
recorded as `timeout` or `crash` and the rest of the design still runs. An action that passes in the baseline and
times out is listed but not judged (and stays passing when blessed): the slowest actions take 40 to
80 s on a busy runner and land either side of the limit from night to night. Loading a design, before
its first action, may take up to 300 s (the largest take several times longer on a loaded CI
runner than here). None did in this run.

Pieces that share a name (`#2`, `~3`) are numbered by where they lie, and the numbers of pieces
lying almost level can swap between runs (#569). The ratchet therefore counts passes per family of
pieces (the name without its numbers) and samples whole families, so a swap is not a regression.

Each open cause below has a small reproduction among the ignored tests at the end of
`action_matrix.rs` (`cargo test --profile ci -p solidcraft-cli --test action_matrix -- --ignored`
lists which still fail; skip `action_matrix_` to leave out the matrix itself).

## The corpus

- `corpus/*`: the old design files of [`apps/solidcraft-cli/tests/corpus`](../apps/solidcraft-cli/tests/corpus/).
- `examples/*`, `regressions/*`: the scripts in [`examples/`](../examples/).
- `oracle/*`: the 76 reference recipes, rebuilt (`solidcraft-cli recipe`).
- `demo/enclosure_steps`: the electronics enclosure built over MCP;
  `demo/pi-case`: the Raspberry Pi 4 case of the 0.6 README demo, built through the desktop UI
  and saved by `examples/demo/enclosure_ui_mcp.py --story pi-0.6 --save`;
  `demo/pi-case-0.8`: the 0.8 README demo's case (`--story pi`), with a rounded rim, engraved
  text and a PETG material;
  `demo/logo-up-down-join`: the SolidCraft logo as it was reported in #954, one sketch extruded
  up and down and joined.
- `typical/*`: [`tests/action_matrix/designs`](../tests/action_matrix/designs/), parts made the
  ordinary way: a stacked extrude, a filleted and shelled box, a revolved knob, a plate with a
  counterbored hole patterned 3 × 2 and a chamfered outline, a sheet metal L bracket with holes, a
  join and a cut of three bodies, a loft on a plate, a sweep, and one sketch of squares drawn edge
  to edge extruded up and down and joined (`up-down-join`).

## The actions

Each one runs on a scratch copy of the design. It passes when the command works, the timeline
has no new errors, and any sketch it makes is attached to its face (healthy, not a legacy face
sketch found again by a point). A face pick is what the desktop sends: the world body, the face
index, the occurrence path and a point inside the face; never `fixed`.

| Entity | Action | Command and parameters |
|---|---|---|
| planar face | `face.sketch` | `sketch.create` on the face |
| | `face.offset_plane` | `construct.plane.offset`, 10% of the body's size |
| | `face.angle_plane` | `construct.plane.perpendicular` through the face's longest straight edge |
| | `face.midplane` | `construct.plane.midplane` with the nearest opposite parallel face |
| | `face.press_pull`, `face.extrude` | `solid.press_pull` / `solid.extrude` (`face`, join), 5% of the size (0.5 to 5 mm) |
| | `face.hole` | `solid.hole` well inside the face (0.8 × the way to its nearest edge, at most 6 mm), through all |
| | `face.draft_neutral` | `solid.draft` of the walls square to the face, 2°, the face's plane neutral |
| | `face.shell` | `solid.shell` removing the face, 1% of the size (0.1 to 0.5 mm) |
| cylinder, cone, torus, sphere | `curved.axis` | `construct.axis.through_cylinder` (not spheres) |
| | `curved.tangent_plane` | `construct.plane.tangent` |
| edge | `edge.fillet`, `edge.chamfer` | the edge alone, 10% of the shortest edge at its ends (not tangent edges, where there is nothing to blend) |
| | `edge.axis` | `construct.axis.edge` (straight edges) |
| | `edge.normal_plane` | `construct.plane.along_path`, halfway |
| | `edge.project` | a sketch on the origin plane it shows longest in, `sketch.project` |
| vertex | `vertex.point` | `construct.point.vertex` |
| | `vertex.plane_three_points` | `construct.plane.three_points` with two neighbours along its edges |
| body | `body.move`, `body.copy`, `body.mirror`, `body.pattern` | `solid.move` 10 mm, and with `copy` beside it; `solid.mirror` about YZ; `solid.pattern.rectangular` 2 × |

Sketching on a cylinder's cap is covered by `face.sketch` on the planar cap. Bodies are capped at
120 faces, 240 edges and 120 vertices each. Entities are named by their persistent names
(`F2:top`, edges `face|face`, vertices by the faces around them).

**The edit round**: one pass in six also changes a driving parameter (the first user parameter,
else a feature's, by 10%, one that the design itself survives) and checks the new feature still
resolves; it is recorded as `<action>+edit`.

## Results

**30567 of 35576 actions pass (85.9%)** on 104 designs; the edit round passes 1370 of 1386.

### By category

| Category | Actions | Pass | Rate |
|---|---:|---:|---:|
| body | 472 | 471 | 99.8% |
| curved | 1848 | 1814 | 98.2% |
| edge | 18763 | 17469 | 93.1% |
| face | 9323 | 5679 | 60.9% |
| vertex | 5170 | 5134 | 99.3% |

### By action

| Action | Actions | Pass | Rate | Edit round |
|---|---:|---:|---:|---:|
| `body.copy` | 118 | 118 | 100.0% | 6 / 6 |
| `body.mirror` | 118 | 118 | 100.0% | 1 / 1 |
| `body.move` | 118 | 118 | 100.0% | 3 / 3 |
| `body.pattern` | 118 | 117 | 99.2% | 8 / 8 |
| `curved.axis` | 906 | 884 | 97.6% | 39 / 39 |
| `curved.tangent_plane` | 942 | 930 | 98.7% | 44 / 44 |
| `edge.axis` | 2897 | 2846 | 98.2% | 140 / 140 |
| `edge.chamfer` | 3465 | 2863 | 82.6% | 122 / 123 |
| `edge.fillet` | 3465 | 2896 | 83.6% | 126 / 126 |
| `edge.normal_plane` | 4468 | 4413 | 98.8% | 190 / 192 |
| `edge.project` | 4468 | 4451 | 99.6% | 223 / 223 |
| `face.angle_plane` | 1004 | 980 | 97.6% | 51 / 51 |
| `face.draft_neutral` | 895 | 246 | 27.5% | 8 / 8 |
| `face.extrude` | 1123 | 465 | 41.4% | 25 / 26 |
| `face.hole` | 1123 | 726 | 64.6% | 40 / 43 |
| `face.midplane` | 686 | 674 | 98.3% | 25 / 26 |
| `face.offset_plane` | 1123 | 586 | 52.2% | 26 / 26 |
| `face.press_pull` | 1123 | 465 | 41.4% | 25 / 26 |
| `face.shell` | 1123 | 953 | 84.9% | 43 / 45 |
| `face.sketch` | 1123 | 584 | 52.0% | 27 / 27 |
| `vertex.plane_three_points` | 2048 | 2012 | 98.2% | 80 / 85 |
| `vertex.point` | 3122 | 3122 | 100.0% | 118 / 118 |

### By design

| Design | Actions | Pass | Rate |
|---|---:|---:|---:|
| corpus/components-arm | 593 | 354 | 59.7% |
| corpus/issue-489-stacked-join | 243 | 227 | 93.4% |
| corpus/joints-hinge | 344 | 258 | 75.0% |
| corpus/m0-box-fillet-hole | 170 | 158 | 92.9% |
| corpus/m0-revolve-primitives | 344 | 300 | 87.2% |
| corpus/modern-path-pattern | 299 | 294 | 98.3% |
| corpus/modern-plastic | 830 | 634 | 76.4% |
| corpus/modern-sheet-metal | 394 | 273 | 69.3% |
| corpus/move-bodies | 262 | 262 | 100.0% |
| corpus/naming-split-pieces | 285 | 224 | 78.6% |
| corpus/split-body-pieces | 304 | 280 | 92.1% |
| demo/enclosure_steps | 1070 | 991 | 92.6% |
| demo/pi-case | 1780 | 1500 | 84.3% |
| examples/bracket | 202 | 191 | 94.6% |
| examples/sensor_enclosure | 2020 | 1852 | 91.7% |
| oracle/01-box | 131 | 131 | 100.0% |
| oracle/02-cylinder | 41 | 41 | 100.0% |
| oracle/03-box-through-hole | 155 | 151 | 97.4% |
| oracle/04-revolve-vase | 101 | 79 | 78.2% |
| oracle/05-taper-extrude | 95 | 87 | 91.6% |
| oracle/06-symmetric-extrude | 131 | 131 | 100.0% |
| oracle/07-fillet-all-edges | 249 | 249 | 100.0% |
| oracle/08-chamfer | 206 | 188 | 91.3% |
| oracle/09-shell | 274 | 265 | 96.7% |
| oracle/10-rect-pattern-holes | 419 | 415 | 99.0% |
| oracle/11-circular-pattern-flange | 209 | 201 | 96.2% |
| oracle/12-mirror-feature | 193 | 181 | 93.8% |
| oracle/13-combine-join | 231 | 202 | 87.4% |
| oracle/14-combine-cut | 160 | 146 | 91.2% |
| oracle/15-combine-intersect | 108 | 78 | 72.2% |
| oracle/16-sweep | 64 | 56 | 87.5% |
| oracle/17-loft | 90 | 64 | 71.1% |
| oracle/18-hole-features | 286 | 262 | 91.6% |
| oracle/19-l-bracket-constrained | 187 | 187 | 100.0% |
| oracle/20-slot-constrained | 106 | 100 | 94.3% |
| oracle/21-draft | 123 | 99 | 80.5% |
| oracle/22-split-body | 80 | 56 | 70.0% |
| oracle/23-bracket-multifeature | 450 | 381 | 84.7% |
| oracle/24-user-parameters | 155 | 151 | 97.4% |
| oracle/25-sketch-triangle | 102 | 100 | 98.0% |
| oracle/26-sketch-belt | 129 | 125 | 96.9% |
| oracle/27-sketch-trapezoid | 178 | 173 | 97.2% |
| oracle/28-blind-pocket-fillets | 361 | 330 | 91.4% |
| oracle/29-two-sided-and-partial-revolve | 314 | 256 | 81.5% |
| oracle/30-fillet-after-shell | 421 | 406 | 96.4% |
| oracle/31-fillet-l-all-edges | 375 | 371 | 98.9% |
| oracle/32-boss-plate-fillets | 174 | 167 | 96.0% |
| oracle/33-fillet-tangent-chain | 184 | 166 | 90.2% |
| oracle/34-chamfer-then-fillet | 312 | 255 | 81.7% |
| oracle/35-fillet-curved-curved | 73 | 70 | 95.9% |
| oracle/36-corner-blend-two-radii | 242 | 236 | 97.5% |
| oracle/37-pyramid-apex-blend | 169 | 144 | 85.2% |
| oracle/38-coincident-cylinders | 41 | 41 | 100.0% |
| oracle/39-tee-pipe | 119 | 118 | 99.2% |
| oracle/40-box-sphere-torus | 226 | 192 | 85.0% |
| oracle/41-tube-radial-holes | 497 | 495 | 99.6% |
| oracle/42-overlapping-patterns | 1094 | 926 | 84.6% |
| oracle/43-cut-splits-body | 262 | 254 | 96.9% |
| oracle/44-shell-l-bracket | 396 | 390 | 98.5% |
| oracle/45-shell-boss-plate | 335 | 322 | 96.1% |
| oracle/46-shell-two-open-faces | 278 | 268 | 96.4% |
| oracle/47-draft-curved-pocket | 221 | 179 | 81.0% |
| oracle/48-pulley | 298 | 274 | 91.9% |
| oracle/49-helical-sweep | 36 | 18 | 50.0% |
| oracle/50-sweep-tight-bend | 77 | 61 | 79.2% |
| oracle/51-loft-rect-circle-offset | 90 | 64 | 71.1% |
| oracle/52-circular-pattern-fillet | 267 | 226 | 84.6% |
| oracle/53-path-pattern | 299 | 294 | 98.3% |
| oracle/54-mirror-body-fillets | 239 | 209 | 87.4% |
| oracle/55-tapped-hole-cosmetic | 179 | 167 | 93.3% |
| oracle/56-holes-on-curved-face | 143 | 123 | 86.0% |
| oracle/57-hole-pattern-sketch-points | 485 | 456 | 94.0% |
| oracle/58-phone-stand | 635 | 552 | 86.9% |
| oracle/59-enclosure-base | 1029 | 942 | 91.5% |
| oracle/60-pipe-flange | 378 | 356 | 94.2% |
| oracle/61-shaft-keyway | 310 | 231 | 74.5% |
| oracle/62-heat-sink | 1781 | 1524 | 85.6% |
| oracle/63-hinge-knuckle | 432 | 353 | 81.7% |
| oracle/64-sketch-tangent-arc-chain | 173 | 142 | 82.1% |
| oracle/65-sketch-gear-tooth | 170 | 144 | 84.7% |
| oracle/66-sketch-linkage | 201 | 195 | 97.0% |
| oracle/67-sm-base-flange | 131 | 105 | 80.2% |
| oracle/68-sm-edge-flange | 263 | 187 | 71.1% |
| oracle/69-sm-tray | 919 | 599 | 65.2% |
| oracle/70-sm-flange-45 | 261 | 185 | 70.9% |
| oracle/71-sm-custom-rule | 394 | 278 | 70.6% |
| oracle/72-sm-contour-flange | 394 | 273 | 69.3% |
| oracle/73-sm-hem | 395 | 272 | 68.9% |
| oracle/74-sm-unfold-refold | 619 | 439 | 70.9% |
| oracle/75-sm-bracket-holes | 479 | 323 | 67.4% |
| oracle/76-sm-thickness-variant | 263 | 187 | 71.1% |
| regressions/join_struts_on_bore_wall | 544 | 424 | 77.9% |
| regressions/loft_cut_beside_round_hole | 386 | 305 | 79.0% |
| regressions/oblique_cut_of_cylinder | 306 | 269 | 87.9% |
| regressions/oblique_cut_of_cylinder_hang | 306 | 270 | 88.2% |
| regressions/shallow_loft_cut_from_top_face | 307 | 264 | 86.0% |
| typical/filleted-shelled-box | 402 | 386 | 96.0% |
| typical/holes-pattern-chamfer | 536 | 480 | 89.6% |
| typical/loft | 218 | 180 | 82.6% |
| typical/multi-body-join-cut | 309 | 284 | 91.9% |
| typical/revolved-part | 161 | 153 | 95.0% |
| typical/sheet-metal-bracket | 311 | 230 | 74.0% |
| typical/stacked-extrude | 283 | 275 | 97.2% |
| typical/sweep | 280 | 185 | 66.1% |

## Causes

Every failure, grouped by cause (5013 in all). Faces that can't take an attached sketch fail four
actions each (`face.sketch`, `face.offset_plane`, and `face.press_pull` and `face.extrude`, which
make a legacy face sketch there).

| Failures | Issue | Cause | Actions | Designs | Example |
|---:|---|---|---|---:|---|
| 986 | #556 | Fillet or chamfer of one edge whose ends meet curved, slanted or several faces | `edge.chamfer` 496, `edge.fillet` 490 | 46 | corpus/components-arm `ArmBody (Arm:2)/F4:+x\|F4:+y` |
| 684 | #495 | Sheet metal thickness (rim) faces have no attachment | `face.extrude` 202, `face.press_pull` 202, `face.offset_plane` 140, `face.sketch` 140 | 12 | corpus/modern-sheet-metal `Body1/F2:face:0` |
| 649 | #390 | Draft refuses walls whose neighbours aren't square to the pull (rounds, other walls), or corners that change after the draft | `face.draft_neutral` 649 | 65 | corpus/components-arm `ArmBody (Arm:2)/F4:+x` |
| 384 | #494 | Revolve, Offset Face, Boss and the other plastic features drop the body's face sources | `face.offset_plane` 100, `face.sketch` 100, `face.extrude` 92, `face.press_pull` 92 | 11 | corpus/m0-revolve-primitives `Body1/F2:end` |
| 380 | #493 | Cut walls, hole faces and chamfer faces have no source | `face.extrude` 95, `face.offset_plane` 95, `face.press_pull` 95, `face.sketch` 95 | 17 | corpus/split-body-pieces `Body1 (1)/F4:side:l1` |
| 336 | #557 | Pattern and mirror copies share or lose their faces' sources | `face.extrude` 84, `face.offset_plane` 84, `face.press_pull` 84, `face.sketch` 84 | 6 | demo/pi-case `Body1/F6:top` |
| 285 | #558 | Picks on a moved or pasted component miss (construction geometry, Hole, Press Pull, Project) | `edge.normal_plane` 55, `edge.axis` 51, `vertex.plane_three_points` 36, `face.extrude` 34 | 2 | corpus/components-arm `ArmBody (Arm:2)/F4:+x` |
| 226 | #559 | A through-all Hole along a wall or beside curved faces: the cut fails | `face.hole` 226 | 46 | corpus/m0-revolve-primitives `Body1/F2:end` |
| 155 | #560 | Hole on a sheet metal face is refused | `face.hole` 155 | 12 | corpus/modern-sheet-metal `Body1/F2:face:0` |
| 146 | #561 | Loft, Sweep and Coil faces have no source | `face.offset_plane` 38, `face.sketch` 38, `face.extrude` 35, `face.press_pull` 35 | 10 | oracle/16-sweep `Tube/F3:end` |
| 116 | #431 | Fillet or chamfer along a curved edge: the ball doesn't fit along the edge | `edge.chamfer` 68, `edge.fillet` 48 | 16 | corpus/modern-sheet-metal `Body1/F2:face:14\|F2:face:16` |
| 100 | #492 | A face split into pieces by a later feature | `face.extrude` 25, `face.offset_plane` 25, `face.press_pull` 25, `face.sketch` 25 | 5 | corpus/issue-489-stacked-join `Body1/F2:top#[F2:side:l1,F4:side:l5]` |
| 98 | #491 | A face that a Join merged from two features' faces | `face.sketch` 27, `face.offset_plane` 25, `face.extrude` 23, `face.press_pull` 23 | 12 | corpus/issue-489-stacked-join `Body1/F2:side:l2` |
| 86 |  | Shell's other refusals: a removed face meeting a round as large as the wall, freeform (loft) faces, walls thinner than twice the shell (expected, not filed) | `face.shell` 86 | 22 | corpus/modern-plastic `Body1/F3:face:15` |
| 84 | #566 | Shell of swept and sheet metal bodies: a corner whose faces don't meet the same way | `face.shell` 84 | 12 | oracle/30-fillet-after-shell `Box/F3:inner(F2:start)` |
| 80 | #562 | Press Pull beside rounds and curved neighbours: the join fails | `face.extrude` 40, `face.press_pull` 40 | 13 | demo/enclosure_steps `Enclosure/F2:start` |
| 69 |  | Blend larger than the faces around it (the 10% radius doesn't fit; expected, not filed) | `edge.chamfer` 38, `edge.fillet` 31 | 11 | corpus/modern-plastic `Body1/F2:inner(F1:+x)\|F3:face:19` |
| 56 | #563 | Draft and tapered Extrude drop the faces' sources | `face.extrude` 14, `face.offset_plane` 14, `face.press_pull` 14, `face.sketch` 14 | 3 | oracle/05-taper-extrude `Frustum/F2:start` |
| 48 | #564 | The rim a Shell leaves has no source | `face.extrude` 12, `face.offset_plane` 12, `face.press_pull` 12, `face.sketch` 12 | 8 | demo/pi-case `Body1/F2:top` |
| 18 | #567 | Axis or tangent plane on a drill tip: no face at NaN | `curved.axis` 9, `curved.tangent_plane` 9 | 3 | oracle/18-hole-features `Plate/F5:face:15` |
| 9 |  | Axis through a face the kernel sees as a cylinder that the command doesn't (sensor enclosure slot ends; not filed yet) | `curved.axis` 9 | 1 | examples/sensor_enclosure `Lid/F15:blend(F14:side:l2\|F14:side:l3)` |
| 7 |  | Other failures in the edit round: the hole cut and blend sizes above, and an extrude from the M0 box's top face whose profile isn't found again (not filed yet) | `face.hole+edit` 3, `face.extrude+edit` 1, `face.press_pull+edit` 1, `edge.chamfer+edit` 1 | 4 | corpus/m0-box-fillet-hole `Body1/F2:top` |
| 2 |  | A helix edge projected along its axis is a point (expected) | `edge.project` 2 | 1 | oracle/50-sweep-tight-bend `Bend/F3:side:c1#1\|F3:side:c1#2~2` |
| 1 | #566 | Shell after the edit round: a corner whose faces don't meet the same way after the move | `face.shell+edit` 1 | 1 | demo/pi-case `Body1/F4:inner(F2:start#[…])#[F6:side:c2@F7.2]` |

Where the face history stands (`crates/kernel/src/history.rs`, [face attachments](face-attachments.md)):
a flat face takes an attached sketch when the body carries an exact source certificate for it
(extrude caps and sides, boxes, shells' inner walls, STEP faces, sheet metal panels), or else
when its persistent name follows exact history (a `lineage` certificate): cut walls and floors,
chamfers, pattern and mirror copies, sides a Join merged, revolves, lofts, sweeps, drafts, tapers,
shell rims, the plastic features' faces and the faces around them, and pieces of a split face (by
the piece's own name where Offset Face moved it off the others' plane). Sheet metal panels, their
rims and the flat ends of bends attach by name. What still refuses a sketch is what makes faces
without recording their history (some cuts, the face a Split Body cuts with another body's
face): press-pull and extrude-from-face then fall back to a legacy face sketch found again by a
point.
