Updates: WR-716

#### WR-716 (update, 2026-11-23). `fillet_chamfer_corner` is removed; the chamfer-corner event is the only chamfer corner it classifies

A corner where every blended edge is chamfered now always goes to the blend corner's chamfer event (`crate::blend::blend_corner::chamfer`, from `FilletSession`'s corner build in `crates/solidcore/src/blend/fillet_executive/session.rs`), and the flag is gone. Removed: the field (`crates/solidcore/src/kernel/fixes/fillet_chamfer_corner.md`), its doc lines, `solidcore-ops`' refusal of `ChamferCorner::Facet` without the fix, and the tests' flag-off halves.

- **What stays pinned:** the pinned 3x3 corner's chamfer branch is not deleted. A corner the event declines still builds there, as the retirement plan above says: a chamfer edge that is not straight, a face that is not a plane, a chamfer that is not of constant width. Deleting it would turn those valid corners into refusals. The event's refusals (a mitre point outside the corner, an undetermined flat corner, mixed convexity) are unchanged.
- **The default corner type is the mitre** (`BlendChamferCorner::Mitre`, the session's default). A box corner of three chamfers is exactly the box cut by the three chamfer planes, and it removes nothing the caller did not ask for. `fillet_edges` and `fillet_edges_per_edge` with linear sections at such a corner now build that mitre: before, they reached the pinned branch's non-bevel route and built a triangular facet (or, for unequal chamfers, a four-sided B-spline patch). SolidCraft already ran with the fix on, so its results do not change.
- **Tests:**
  - `wr697_ball_corner::per_edge_linear_chamfers_at_a_corner_are_the_mitre` builds the 2/3/4 corner's mitre with every fix and with none.
  - `solidcore-ops` `blend_chamfer_corner::the_box_corner_s_mitre_and_facet_need_no_fix` builds the mitre and the facet with no fixes set.
  - `wr_657_offset_splits_corner_vertices` pins the offsets and shells of the box with three and with twelve chamfered edges at their mitre corners. `wr_657_closed_forms_replay` recomputes every pinned volume apart from the kernel, as the volume of the convex polytope the faces' moved half-spaces cut out.
  - Rig `3655-chamfer-all-box-edges-pick-order` expects the mitred box, 15000 - 0.5 * 2.5^2 * 330 + 6 * 2.5^3 = 14062.5 (before: the facet's 14052.083333, 8 d^3/12 less). `wr_657_offset_splits_corner_vertices::issue_3655_mitred_box_closed_form` replays it.
