Updates: WR-600

#### WR-600 (update, 2026-10-31). The tangent end is an event of the trace (fork 856b54e)

- **What happens:** with WR-600's change a side edge whose faces meet tangentially at the corner's vertex is of the filleted edge's convexity, so at a mixed-convexity Nx1 corner the rail on its side goes to `SM_FV_RAIL_X_EXTENDED_EDGEUSE`. When that side edge is a tangent seam T (the faces tangent at the vertex but with different normal curvature across it, as in the de3a8c9 peg: the front face y = 0 of a bar 40 x 16 x 10 runs into the cylinder of its half-disc end R 8 along x = 40), the rail actually ends where it crosses T. The end rib was then marched into the point where the fillet surface touches the end face, and the cap edge along T was taken as the intersection of two surfaces tangent along it. A chamfer d 2 of the front top edge, or of the concave arc where the top meets a disc R 8 standing 10 on the end, fails with 1001 (Corner Edge Computation Failure).
- **Fix:** the SolidCraft fork's tangency-labs/smlib@856b54e, not merged upstream. `sm_IsTangentSeamAtVertex` finds T: tangential at the vertex, not the filleted edge's G1 continuation, normal curvatures across it differing by more than `SM_EFF_ZERO_SQRT (1 + |k0| + |k1|)`. At such a side (`bTangentSide`), the rail end is a rail/edge crossing marked as the tangent end, and the end face is the face across T. That face is extended `SM_CT_CINFINITY`, not G1, and the cap edge is `SM_FE_ON_EXTEND_EDGE`. `sm_TangentEndRib` solves the end rib section by section from the tangent end outward, in `t` with `s ~ t^2` for odd contact order. `CalcEdgeOnTangentSeam` takes the cap edge as T's own curve continued as itself. `RailEdgeuseIntersect` runs a tangent seam on past the corner's vertex too, as an exception to WR-643's far-end-only run-on.
- **Rust port:** under the same flag, `Kernel::fixes.fillet_nx1_tangent_side_edge_same_convexity` (nothing is shipped, so the flag is extended rather than split):
  - `is_tangent_seam_at_vertex` and the `tangent_side` reads in `blend/fillet_corner/topology.rs`, with the extension and cap-edge types in `topology/nx1_end.rs`;
  - `FilletVertex::tangent_end`;
  - the rib in `blend/fillet_geom/tangent_end_rib.rs`, dispatched from `calc_fillet_int_side_face`;
  - `calc_edge_on_tangent_seam` in `blend/fillet_geom/p5.rs`;
  - the run-on exception in `blend/fillet_solver/p2.rs`. It lives inside WR-643's code, so it acts only with `fillet_run_on_past_far_end_only` on too.

  On a cylinder the side edge's UV trim is a B-spline, which clamps, so the rail on the disc's wall finds its crossing with T past the vertex only with WR-654's `fillet_run_on_bspline_trims` as well. The fork's profile has all three on.
- **Results** on the peg, with those flags plus `fillet_chamfer_cone_on_arc`, `fillet_torus_span_from_start_section` and `cover_wraps_across_closed_seam`, against closed forms (each section swept along the edge, or about the disc's axis, and cut at the tangent end):

  | Case | Before | After | Closed form |
  |---|---|---|---|
  | front chamfer d 2 | 1001 | 9341.798377085 | 9341.798376871 |
  | concave chamfer d 2 | 1001 | 9457.585734987 | 9457.585731736 |
  | concave round r 2 (WR-600's case) | 9434.384336602 | 9434.384309110 | 9434.384310516 |

  The fork's message gives 9457.585735987 for the concave chamfer, against its quadrature's 9457.585731695. The front round r 2 changes by 5e-6. With WR-600's flag off nothing changes.
- **Not ported here:** the fork's cd47efd (the G2 rib in `t = sqrt(s)`, which needs 0cc8cd4).
- **Test:** `crates/solidcore/tests/fork_856b54e_tangent_end.rs`.
