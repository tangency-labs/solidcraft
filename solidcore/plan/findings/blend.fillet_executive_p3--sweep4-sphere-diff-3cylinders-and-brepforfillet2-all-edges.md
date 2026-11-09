# blend.fillet_executive_p3: sphere_diff_3cylinders and brepForFillet2 all-edges at r=0.1 disagree (2026-10-26)

Unit: `blend.fillet_executive_p3` (real-solid end-to-end runner). Fixtures:
`fixtures/blend.fillet_executive_p3/sweep_round4.json`, listed in
`crates/difftest/known_mismatches/blend.fillet_executive_p3.toml`. Both are file
sources (`Fillet/<file>.smb`), every edge, radius 0.1, circular, G1, thumbweight 1.
Both disagree when run alone (not the ND-3 `sdStaticStep` order effect, which the p3
runner now neutralises). Investigation is a follow-up.

## sweep4-sphere_diff_3cylinders-all-r0.1

Both sides succeed. Status, counts, topology, every vertex/edge/face record of the
result geometry, and (checked with a temporary debug record) every edge use's UV-trim
curve interval and samples at 0.17/0.37/0.83, and every face's loose bounding box, agree
within 1e-9. Only the mass properties differ:

| | C++ | Rust |
|---|---|---|
| area | 38235.606154303496 | 38235.00487528196 |
| volume | 169492.711686787 | 169494.85130010243 |
| centroid x | -4.460296831157796e-3 | -4.216528596161486e-3 |

Per face (`SmApiFaceComputeArea`, accuracy 1e-4), the three cone faces 3, 6 and 7
differ: 3158.7134622668486 / 3158.6886066743787, 3158.6214810468996 /
3158.5874527586147, 3159.334991631436 / 3158.7201289796844; the other five agree.
The integrator itself agrees on identical input: the C++ result written with
`WriteToFile` and read back gives the same per-face areas in C++ and in Rust (and
different ones from the in-memory C++ body, so the area is sensitive to representation
details the record does not carry). Suspected: the fillet's trimmed cone faces differ
in a UV-trim curve's representation (knots / control points) or another face-level
datum the integrator reads.

## sweep4-brepForFillet2-all-r0.1

C++ succeeds (status 0, 27 vertices, 54 edges, 29 faces, area 1433.6415218666718).
Rust returns `SM_ERR_INVALID_INPUT` (1007) and leaves a 37/48/16 body (area
1441.0696785133293). Where the Rust executive first refuses is not yet located.

Found in the round-4 stub sweep of the fillet executive p1 integration branch.
