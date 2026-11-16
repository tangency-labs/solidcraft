Updates: WR-520

#### WR-520 (update, 2026-11-02). A drafted arc's wall is its exact cone (fork 3ae7bfa)

- **Location:** the fork's `sm_CreateCurvatureKeepingWalls`, `sm_DraftSeamPcurve` and the new
  `sm_DraftCone` (`source/SMLib/src/SmPrimitiveCreation.cpp`, tangency-labs/smlib@3ae7bfa,
  smlib#116); not in the pinned SMLib.
- **What happens:** the curvature-keeping walls built every wall ruled between its ends
  (`CreateRuledSurface`), so an arc's wall was a B-spline that lies on its cone but is no
  `SmCone`. In the fork's C++ the B-rep copy's `CopyAndAddAnalytics` leaves it a B-spline, so
  a union cannot merge it with a coincident `SmCone` face: the drafted tangent peg's bar wall
  and disc wall stay two faces, and the vertex where the front top edge meets the concave arc
  gets four edges. (The port's `copy_and_add_analytics` already took that ruled wall for a
  cone, over the ruled surface's domain [-0.001, pi + 0.001] x [0, 1], so this union merged
  in the port before the change as well once WR-599's addendum is in.)
- **Fix (fork 3ae7bfa):** an arc's wall whose height runs along its axis is the `SmCone`
  about that axis (`SmCone::CreateCanonical` with swapped u/v, inside out where the height
  runs against the axis, `AdjustSTEPUVDomain` to the wall's angles). The wall's spans are the
  cone's equal V spans. A seam's pcurve on it is cut where the seam crosses a span's end, and
  each piece is the exact rational quartic of its span. The arc's half-turn refusals no longer
  arise for a cone wall.
- **Rust port:** `primitive/primitive_creation/p2/taper_walls.rs` (`draft_cone`,
  `seam_pcurve`, both `curvature_keeping_walls` paths, i.e. with and without WR-571's
  `taper_wall_closes_at_apex`). There is no `Kernel::fixes` field, as for WR-520 itself: only
  `OffsetCornerType::CurveExtension` (not in the pinned C++) and WR-521's opt-in tangent rule
  reach these walls, so the default results and difftest are unchanged. Regression test:
  `crates/solidcore/tests/fork_3ae7bfa_drafted_cone_wall.rs`
  (`the_bars_arc_wall_is_its_exact_cone`: one cone face within 1e-12 of the drafted cone, of
  semi-angle 5 degrees, over the fork's natural domain [0, 1] x [0, 10]; the bar's counts
  6/12/8 and closed-form volume, as the fork at dda881f run locally).
