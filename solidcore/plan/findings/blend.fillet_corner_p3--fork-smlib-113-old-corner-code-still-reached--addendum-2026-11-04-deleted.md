# blend.fillet_corner_p3: the old corner code the blend corner replaced is deleted (2026-11-04)

Updates: plan/findings/blend.fillet_corner_p3--fork-smlib-113-old-corner-code-still-reached.md

- **Units:** `blend.fillet_corner_p1`, `blend.fillet_corner_p2`, `blend.fillet_corner_p3`,
  `blend.fillet_executive_p1` (`TestAndConvertBigRadFillet`), `blend.fillet_geom_p5`.
- **Change:** tangency-labs/solidcore#2400. `fillet_blend_corner` is always on (WR-697's
  addendum of 2026-11-04), so the pending deletions of the record and of its 2026-11-02 addendum
  are made, one commit each, each proved unreachable from the blend corner's classification.

## Deleted

| Piece | Why nothing reaches it |
|---|---|
| The flag's sites: the closed Nx1 corner's `GetTangentFilletSolver` and `AdjustTangentCorner` (`tangent_fillet_solver_nx1_closed*`, `adjust_tangent_corner_nx1_closed`, `nx1_closed_tangent_split`), `calc_cross_section`'s `NByOneClosed` clauses, the pinned classification and every `if !fillet_blend_corner` branch | they ran only without the flag |
| `SmFillet1x1Corner`, `SmFillet2x2Corner` (`FilletCornerKind::OneByOne`, `TwoByTwo`, `make_1x1_corner_topology`, `make_2x2_corner_topology`, `make_cross_section_vertices`, their tangent solvers) | `create` makes those vertices the Join (543d25a, 68e039c): no constructor is left |
| `make_nx1_closed_corner_topology`'s G1 test and branches | `create` runs the same test on the same edge and tolerance and makes a G1 closed edge the Join (caa67d0) |
| `make_nx2_corner_topology`'s G1 and blending branches | `Classify` runs the same tests and makes those pairs the Join (e4083c8) |
| `TestAndConvertBigRadFillet` (`test_and_convert_big_rad_fillet*`, `session/big_radius.rs`) | it ran only at a blending Nx2 corner past the three-edge return; the events that blend adjust nothing, and the Turn blends only at its three-sided face (fe980d7) |
| `SmFilletNx2Corner::AdjustTangentCorner` (`adjust_tangent_corner*`, `is_nx2_tangent_adjustment_corner`, the tangent split's `TangentSplitOps` / `TangentSplitEnv`) | every two-blend corner carries a blend event, and every event's tangent adjustment is the base class's success (543d25a, 53c47b8, 34288e5) |

## Kept

- **The N x N setback paths** #2365 kept (3fc2814fb, e6c3a6aec): the Setback does not build every
  corner they do. A setback asked with `SetSetBackDist` alone at a corner the Ball takes is
  refused (`asked_setback_not_built`); marking the vertex with `AddSetBackVertex` first, as
  SolidCraft does, makes it the N x N corner.
- **The closed Nx1 corner's angle case:** an edge meeting itself at an angle is still that
  corner, until #2349 replaces it.
- **The convex N x N chamfer branch:** the chamfer event replaces it only with
  `fillet_chamfer_corner` (WR-716), still opt-in; with it off the pinned chamfer corner is the
  default's.
- **The NByOne, Open and Degenerate kinds:** the End's three constructions, moved unchanged
  (53c47b8).
- **`FilletCornerType::OneByOne` and `TwoByTwo`:** upstream's enum codes.
