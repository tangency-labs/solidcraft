# blend.fillet_corner_p3: smlib#113's closed-edge corner after step 4d: unreachable under the blend corner, still the pinned default's (2026-11-02)

Updates: plan/findings/blend.fillet_corner_p3--fork-smlib-113-old-corner-code-still-reached.md

- **Units:** `blend.fillet_corner_p3` (`SmFilletNx1ClosedCorner::MakeCornerTopology`,
  `blend::fillet_corner::topology::closed_nx1`), `blend.fillet_corner_p2`
  (`SmFilletNx1ClosedCorner::GetTangentFilletSolver`, `AdjustTangentCorner`),
  `blend.fillet_geom_p5` (`SmFilletEdge::CalcCrossSection`).
- **Fork change:** the last two commits of tangency-labs/smlib#113, caa67d0 and 2ac7a17.
- **Port:** PR #2436, issue #2342. Follows #2353 (the other four commits) and #2307 (step 4d,
  merged at 379f9dafe).

## What changed since the record

The record's table said caa67d0 and 2ac7a17 were reached with `fillet_blend_corner`, because
`Create` had no closed Join. #2307 ported 543d25a. With the flag, `FilletCorner::create` now makes
a closed blended edge with one to three edge uses that meets itself G1 (`test_g1_continuity` at
the solvers' tangency tolerance) the blend corner's Join. Only an edge meeting itself at an angle
stays the closed Nx1 corner. So, with the flag:

| Fork commit | Rust | With `fillet_blend_corner` | With the default flags |
|---|---|---|---|
| caa67d0 | `make_nx1_closed_corner_topology`'s G1 test and its G1 branches (mate vertex, `CROSS_SECTION` edge, blending) | not reached: the test repeats `create`'s, on the same edge, vertex and tolerance, so it is always false | reached (pinned) |
| caa67d0 | `tangent_fillet_solver_nx1_closed` | always `None`: same test | reached |
| caa67d0 | `adjust_tangent_corner_nx1_closed` | acts only where the edge's end tangents are parallel within 2 degrees, which `create` made a Join at any tangency tolerance of 2 degrees or more (10 by default): at most at the rounding of an angle of exactly 2 degrees | reached |
| 2ac7a17 | `calc_cross_section`'s `NByOneClosed` clauses | not reached: the corner makes no `CROSS_SECTION` edge and is never blending | reached |

`sm_CheckSeamOut` and `sm_SetSurfaceClosure` stay: each is one Rust function shared with
`SmFilletGeom.cpp`'s copy, which `fillet_geom::p3` calls (as the record says).

## What #2436 does

The code is still the pinned default's, which AGENTS.md section 4b keeps until the flag becomes
the default, so nothing is deleted. Under `fillet_blend_corner` the closed corner is the fork's
angle case alone: the topology doesn't test for G1 (`g1_case` false), the tangent solver is
`None` (the base class's answer), the tangent adjustment does nothing (the base class's success),
and `calc_cross_section`'s closed-corner clauses are the closed Join's alone.

## Pending deletion (when `fillet_blend_corner` becomes the default)

- `make_nx1_closed_corner_topology`: the G1 test and its branches (`Mate` vertex, `CrossSection`
  edge, `blending = true`); the corner's `g1_case` for this kind.
- `tangent_fillet_solver_nx1_closed` and `_with_lookup`, and the `NByOneClosed` arm of
  `tangent_fillet_solver_with_lookup`.
- `adjust_tangent_corner_nx1_closed`, `nx1_closed_tangent_split`, the `nx1_closed` callback of
  `adjust_tangent_corner_with` and its session adapter.
- `calc_cross_section`: the `NByOneClosed` blending clause and the `NByOneClosed` terms of the two
  closed-surface tests.
