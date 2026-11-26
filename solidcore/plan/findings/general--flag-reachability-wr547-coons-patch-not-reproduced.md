# General: WR-547 `corner_blend_coons_patch_tangent_to_sides` has no operation that shows the fix (2026-10-31)

- **Flag:** `Kernel::fixes.corner_blend_coons_patch_tangent_to_sides` (WR-547, fork tangency-labs/smlib@cae8618). The guard is in `nurbs/b_spline_surface_n_lib/p2.rs` `corner_blend_4`, after `create_coons_patch`.
- **Audit:** `plan/findings/audits/flag-reachability-2026-10-31/` classifies it as D. Its only test, `tests/wr_547_coons_patch_tangent_to_sides.rs` (the L-notch, all three rounds r 2), passes without ever reaching the guard: the port builds that corner without the four-sided Coons path.
- **Tried (instrumented build of origin/main 9112d3a0e, `fillet_edges_per_edge`, circular sections; each run with the flag on and off):**
  - The L-notch (20 x 20 x 10 block less the quarter x, y in 10..20), concave vertical round / two convex top rounds: 2/2, 3/2, 4/2, 5/2, 2/3, 2/5, 5/5.
  - A box corner with three convex rounds: 2/3/4, 2/2/5, 5/2/2.
- **Result:** only the notch with a concave r 5 and convex r 2 reaches the guard. Its worst seam angle between curved faces is 0.007 degrees with the flag on and off, so the fix changes nothing measurable. Every other case is at most 0.014 degrees either way and never reaches the guard.
- **Consequence:** the record's measurable (14.7 degrees in the fork's L-shaped box, under 1 degree with the fix) is not reproduced: the port's default patch is already tangent where it is built. No real-operation test can fail with the flag forced off until a geometry is found where the default Coons patch is not tangent. The record's other trigger, SolidCraft rig fillets/1207 (a bar's top-edge round meeting a boss's base round, at r 2 and 5), is not public.
- **Waits on:** that geometry (or the rig part). No other fork fix is known to be missing in front of the branch.
