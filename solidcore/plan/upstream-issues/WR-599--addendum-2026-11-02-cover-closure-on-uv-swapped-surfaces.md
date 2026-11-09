Updates: WR-599

#### WR-599 (update, 2026-11-02). The cover reads a UV-swapped surface's closure in the step's direction (fork dda881f)

- **Location:** the fork's `SmSurface::CoverCoincidentSurface` wrapped-seam rule
  (tangency-labs/smlib@16bf1cb), `source/SMLib/src/SmSurface.cpp`; changed by
  tangency-labs/smlib@dda881f (smlib#116). Not in the pinned SMLib.
- **What happens:** the rule tests whether this surface is closed in u and in v
  (`IsClosed`, which reads the NURBS), but the boundary walk steps in STEP parameters. A
  surface of revolution whose NURBS swaps them (`GetSwapUVBit`: an `SmCone` built with
  `bSwapUV`, as the drafted disc's wall is) wraps in STEP u where its NURBS closes in v. So a
  step across its seam was read against the wrong direction and refused, and the union of
  the drafted tangent peg (whose bar wall is a cone since WR-520's addendum of the same date)
  kept the bar's cone wall and the disc's cone wall as two faces, with four edges at
  V = (40, 10 tan 5, 10).
- **Fix (fork dda881f):** with the surface's swap bit set, STEP u's closure is the NURBS's v
  closure and STEP v's is u's.
- **Rust port:** under the same option, `Kernel::fixes.cover_wraps_across_closed_seam`
  (`surface/surface/p3.rs`). The swap bit is `swap_uv` of a surface of extrusion or
  revolution, or of the revolution under a cone, sphere or torus; every other kind has none,
  as the B-spline default `GetSwapUVBit` returns `FALSE`. Regression test:
  `crates/solidcore/tests/fork_3ae7bfa_drafted_cone_wall.rs`. With the flag on (and WR-598's
  `delete_vertex_joins_seam_iso_curve`, which joins the seam vertex the merge leaves), the
  drafted peg is 7 faces, 15 edges and 10 vertices, with one cone face, three edges at V and
  the closed-form volume, as the fork at dda881f run locally. Without WR-598 it is 7 faces,
  16 edges and 11 vertices. With the flag off it is 8 faces with four edges at V (upstream
  refuses every crossing). `the_drafted_peg_has_one_cone_face` fails without this change.
