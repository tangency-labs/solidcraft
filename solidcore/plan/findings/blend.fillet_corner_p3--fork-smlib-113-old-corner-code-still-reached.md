# blend.fillet_corner_p3: smlib#113 deletes old corner code that solidcore still runs, in the default and under the blend corner (2026-11-02)

- **Units:** `blend.fillet_corner_p3` (`SmFilletNx2Corner::MakeCornerTopology` in
  `blend::fillet_corner::topology`, `AdjustTopology`, `AdjustTangentCorner`, the blend corner's
  dispatch), `blend.fillet_corner_p2` (`SmFilletNx1ClosedCorner::AdjustTangentCorner`,
  `GetTangentFilletSolver`), `blend.fillet_executive_p1` (`TestAndConvertBigRadFillet`,
  co-ported in `blend::fillet_corner::p3`), `blend.fillet_geom_p5` (`SmFilletEdge::CalcCrossSection`).
- **Fork change:** tangency-labs/smlib#113, merged into replay/1.0.1 at 2ac7a17 (26b18e4, fe980d7,
  e4083c8, 34288e5, caa67d0, 2ac7a17). It deletes old corner code (+30 / -1786) with every capi log
  the same before and after, because in the fork nothing reaches that code any more.
- **Port:** PR #2353, issue #2342.

## Why the fork could delete it

The fork's reasons rest on two steps before it:

- **Step 4b** (smlib#79, `SmBlendCorner::Classify`): two blends of one convexity among three or four
  edges are a Join when their edges are G1 at the tangency tolerance, or meet at 5 degrees or less at
  a face corner of the other convexity. Otherwise they are a Turn.
- **Step 4d** (smlib#79, 68e039c and 543d25a): the Join has its own build (`MakeJoinTopology`, the
  shared section), and `SmFilletCorner::Create` makes a closed blended edge that meets itself G1 the
  Join too.

With both, the old G1 branches only ever saw pairs that `Classify` had already called Turns:

- the two-blend topology's G1 and three-edge branches;
- the big-radius conversion, which runs only on a blending corner;
- the tangent adjustment, which acts only on tangents parallel within 2 degrees, when every caller
  sets the tangency tolerance at 2 degrees or more;
- the closed-edge corner's G1 branches and tangent adjustment;
- `CalcCrossSection`'s clauses for a blending closed-edge corner.

`SmFilletEdge::AdjustCrossSection` had no caller at all.

## What solidcore runs (main at b0e925d6c, and again at fdc55bbb4)

| Fork commit | What it deletes | Rust | Reached with the default flags | Reached with `fillet_blend_corner` |
|---|---|---|---|---|
| 26b18e4 | `SmFilletEdge::AdjustCrossSection` | never ported (no caller at the pin: `harness/callgraph/unused_functions.txt`; `kernel.rs`, WR-685) | no | no |
| fe980d7 | `TestAndConvertBigRadFillet` and its call | `FilletExecutiveData::test_and_convert_big_rad_fillet`, called by `adjust_topology_nx2_with` on a blending corner | yes: a G1 pair at the pinned Nx2 corner | yes: the Join, built by the Nx2 port |
| e4083c8 | the two-blend topology's G1 test, its three-edge branch and its four-edge blending branch | `make_nx2_corner_topology` | yes (pinned Nx2 corner) | yes: the Join of three or four edges (mate vertex, `CROSS_SECTION` edge, blending), and the three-edge branch's mitre is the Mitre turn's (the fork's 48a8a9f split it out bit for bit as before) |
| 34288e5 | `AdjustTwoBlendTangentCorner` | `adjust_tangent_corner_nx2` | yes | yes: the Join at four edges |
| caa67d0 | the closed-edge corner's G1 branches, `GetTangentFilletSolver`, `AdjustTangentCorner`, its file statics | `topology::closed_nx1`, `adjust_tangent_corner_nx1_closed`, `tangent_fillet_solver_nx1_closed` | yes | yes: `Create` still makes a closed edge meeting itself G1 an Nx1 closed corner (543d25a not ported) |
| 2ac7a17 | `CalcCrossSection`'s closed-corner clauses | `fillet_geom::p5` (`NByOneClosed` with `is_blending_corner`) | yes | yes (as caa67d0) |

`sm_CheckSeamOut` and `sm_SetSurfaceClosure` (caa67d0's file statics) are one Rust function each,
shared with `SmFilletGeom.cpp`'s identical copies (`fillet_geom::p1`), which `fillet_geom::p3` still
calls.

So there is nothing in the Rust to delete. Section 4b of AGENTS.md (the default is the pinned
C++) keeps all of it for the default. Under `fillet_blend_corner` the blend corner is the Nx2
port with `blend` set (`blend_corner.rs`, Deviations). Until step 4d is ported (#2307), that port
builds the Join, so the G1 branches are the flag's code as well.

## What #2353 ports

What the fork's e4083c8 and 34288e5 change is the Mitre turn at four edges. In the fork,
`MakeTwoBlendTopology` and `AdjustTwoBlendTangentCorner` are reached only there: `MakeCornerTopology`
sends the Ball, Join, End, Setback, Swept turn and three-edge Mitre turn elsewhere. Solidcore
already routes that corner the fork's way, so #2353 ports those two commits for it, under
`fillet_blend_corner` (WR-697):

- **e4083c8:** `make_nx2_corner_topology` doesn't test a four-edge Turn for G1 or mixed convexity.
  It's the two blends mitred: bevel, not blending, one `FILLET_X_FILLET` edge, `g1_case` left
  false.
- **34288e5:** `adjust_tangent_corner_with` makes no tangent adjustment at a Turn.
- **fe980d7:** nothing more. A four-edge Turn is never blending now, so it never reaches the
  big-radius conversion. The pinned default and the flag-on Join still call the conversion.

Results change only where the fork's do:

- **A four-edge Turn whose edge tangents can't be measured.** The pinned test reads an
  uninitialised angle there. `Classify` takes 90 degrees and calls the pair a Turn. Until now the
  port returned `InvalidInput` ("edge angle at the corner is unset") when the mixed-convexity test
  read the angle. Now the pair is mitred, as in the fork.
- **The tangent adjustment at a Turn whose tangents are within 2 degrees.** This can happen only
  at the rounding of an angle of exactly 2 degrees under a 2-degree tolerance.

The default doesn't change.

## What waits for step 4d (#2307)

- **caa67d0 and 2ac7a17:** the closed-edge corner's G1 branches and tangent adjustment, and the
  cross section's closed-corner clauses. They can be ported, under the flag, once a closed edge
  meeting itself G1 is the Join (543d25a).
- **The Join's share of e4083c8, fe980d7 and 34288e5:** none is needed. Once 68e039c routes the
  flag-on Join to its own build, the dispatch no longer reaches these branches for it.

## Checks (this PR)

- **A pyramid apex and a pyramid pocket's floor.** Two opposite slant edges are rounded at each,
  which makes four-edge Turns, convex at the apex and concave at the floor. They are rounded at r
  0.5, 1 and 2, with and without the flag. Every volume (to the bit), face count and `check_brep`
  line is the same before and after the change
  (`crates/solidcore/tests/wr697_four_edge_turn.rs`). A local print in the new branch (not
  committed) showed it taken at the apex and the floor in every flag-on case. No existing test
  builds a four-edge Turn.
- **Every integration test that turns `fillet_blend_corner` on.** The output (`--nocapture`, one
  thread) is the same before and after, apart from timings: 21 files on b0e925d6c, and 23 after
  merging main at fdc55bbb4. The Setback and the Mixed corner, which main added, keep their own
  dispatch ahead of the Turn's.
