Updates: WR-520

#### WR-520 (update, 2026-11-14). A draft's sections that touch within rounding touch

- **Location:** the fork's `sm_DraftCrossing` (`source/SMLib/src/SmPrimitiveCreation.cpp`,
  tangency-labs/smlib 54148db); not in the pinned SMLib.
- **What happens:** the crossing of two walls' sections at a height is refused when the
  line/circle discriminant or the circle/circle `h^2` comes out negative, as computed. Where a
  profile's line or arc touches an arc at a 0 degree tip, both are about 0 exactly and of either
  sign as computed, so the draft from that tip is refused (`SM_TCR_APART`) or built depending on
  rounding. A window whose arc touches its side line, split by the app into two horns at the
  touching point and drafted 15 degrees inward, built its upper horn and refused its lower one:
  the discriminant there is +2.8e-15 exactly for the given doubles and -2.18e-11 as computed.
- **Fix (solidcore):** the sign is decided against a certified rounding bound (Higham's gamma):
  apart only where the bound proves it, one touching point otherwise; a positive discriminant
  is used as before, so every draft that built before is unchanged. The circle/circle case
  decides from `(ra - rb)^2 <= D^2 <= (ra + rb)^2`, which is well conditioned.
- **Rust port:** `primitive/primitive_creation/p2/taper_walls.rs` (`crossing`, `touch_bound`,
  `circles_may_meet`). Regression tests:
  `crates/solidcore/tests/fork_3ae7bfa_drafted_cone_wall.rs`
  (`a_horn_drafted_from_its_touching_tip_builds`: 5 faces, 9 edges, 6 vertices, the volume in
  closed form) and the unit test `two_circles_touching_within_rounding_touch`.
