Updates: WR-442

#### WR-442 addendum (2026-10-26): an opt-in fix in the Rust port
- **Reported independently:** by jyviko (tangency-labs/solidcore#478). Fixed in the SolidCraft fork (branch r-booleans, 0993713).
- **Fix:**
  1. After the stitch, take each solid region that has one shell whose mate lies in the infinite region.
  2. From a point of its outside shell, fire a ray (as `TestClosureSplitRegion`'s shell nesting does) at every other such solid's inside shell.
  3. A solid inside exactly one other, which is itself inside none, becomes a cavity: its outside shell moves into that solid's region, its region is marked void, and its faces are swapped to point into the cavity.

  Deeper nesting is left as built.
- **Rust port:**
  - `crates/solidcore/src/topology/brep/p6.rs` (`nest_closed_shells_as_cavities`), called from `PrimitiveCreation::create_rotational_sweep` (`crates/solidcore/src/primitive/primitive_creation/p3/sweeps.rs`) after the stitch.
  - It is opt-in via `Kernel::fixes.revolve_nested_shells_as_cavities` and off by default, so the default keeps upstream's separate solids and the sweep difftest units keep matching the C++.
- **Test:** `issue_478_full_turn_nested_shell_is_a_cavity` in `sweeps.rs`.
  - WR-442's half ring revolved 360 degrees about X is a manifold solid of volume 4/3 pi (40^3 - 18.5^3).
  - It also checks WR-443's consequence. With that ball as the tool, the block (-4,-4,-4) 8^3 keeps 512, and the block (8,-4,-4) 12 x 8 x 8 keeps 653.3429080270938, the reference's sphere-difference control values.
- Fixes tangency-labs/solidcore#478.
