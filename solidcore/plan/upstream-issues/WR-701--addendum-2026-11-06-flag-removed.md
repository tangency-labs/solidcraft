Updates: WR-701

#### WR-701 (update, 2026-11-06). `swap_uv_keeps_analytic_definition` is removed

The fix is the default (tangency-labs/solidcore#2273, step 4a): `Surface::swap_uv_notifying` of a
cone, sphere, torus or surface of revolution keeps its definition and only rebuilds its polar
converter. The swap is made on an exact copy first (`clone_exact` of the B-spline objects, a
torus's inside-out mark as it is): a net the converter refuses fails the swap with the surface
unchanged and nothing notified. `Plane::swap_uv_notifying` keeps the axes, domain and scales its
`toggle_swap_uv_bit` swapped. The re-derivation from the transposed net after a swap
(`RebuildSTEPFromNURBParameters` with its status ignored) is gone from `SwapUV`;
`Plane::swap_uv_notifying_keeping` folds into `Plane::swap_uv_notifying`. `Surface::reverse` still
rebuilds an analytic from its reversed net, as upstream does.

- **Tests:** `tests/wr701_swap_uv_keeps_analytic_definition.rs` runs on `Kernel::default()`; its
  test of upstream's re-derivation is removed. `tests/wr697_swept_turn.rs`
  (`horn_tori_turned_round_stay_horn_tori`) turns the horn tori round with the definition kept and
  checks the fork's af7a4d9 horn-torus rebuild through `Surface::reverse`, the path that still
  rebuilds a torus from its (swapped) net.
- **Results:** three tests pinned the re-derivation after a swap on the default kernel, and change:
  - `tests/fork_5df5b27_ring_of_arcs.rs`: the ring of 256 arcs' hole walls, turned round by
    `StitchAndOrient`, stay cylinders (semi-angle 0) and the body checks clean at level 2 without
    `cylinder_rebuild_mean_radius` (WR-385), whose rebuild was the trigger; that fix's rebuild
    keeps its own tests in `surface/cone/p2.rs`.
  - `tests/issue_1672_torus_minus_z.rs`: the torus revolved 270 degrees about -z is valid, with
    the Pappus volume to 1e-9, without the torus fixes (WR-408, WR-539).
  - `tests/wr592_torus_rail_on_axis.rs`: with `fillet_torus_rail_on_axis` the corner round's
    surface of revolution no longer fails `SmSurfOfRevolution` #7, which came from its rebuild
    after the swap (`plan/findings/blend.fillet_solver_p2--wr592-revolution-polar-start-angle.md`);
    the corner checks clean.
  - The rig ran with the flag on; the kernel rig at the head is the PR's full gate.
  - Goldens: `surface.surf_of_revolution_p3`'s 165 swap cases keep the state the surface was made
    with (a cylinder built unbounded stays unbounded, as its unswapped copy is), and
    `query.face_p4`'s four natural faces on a swapped cylinder build, where the swap's rebuild
    made the face build fail with 1001; their `check_brep` findings are those of the same unit's
    unswapped cylinders (its faces carry no pcurves). `query.face_p3` (one swapped cylinder)
    and `topology.api_files` (19 written bodies) move with the kept definitions.
