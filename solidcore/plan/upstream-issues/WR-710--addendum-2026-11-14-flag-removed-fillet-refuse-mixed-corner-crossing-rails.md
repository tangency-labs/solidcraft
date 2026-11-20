Updates: WR-710

#### WR-710 (update, 2026-11-14). `fillet_refuse_mixed_corner_crossing_rails` is removed

`SmFilletCorner::Create` refuses a 3x2 mixed-convexity corner whose two filleted edges leave the vertex along one tangent, so their rails cross before the corner, with `SolidError::Unsupported` and the reason `MIXED_CORNER_RAILS_CROSS` (#2256), instead of building a corner patch across them that leaves the faces. The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::fillet_refuse_mixed_corner_crossing_rails`, SolidCraft's preset line and its entry in `fixes.json`. `tests/fork_9bebf34_tangent_end_cases.rs`, which modelled the fork's profile without it, now runs with it.

- **Results:** none change in the goldens or the kernel rig (measured on main a7468ec4f: every goldens unit with this and the other group A flags of the batch on, `blend.fillet_corner_p3` with it alone, and the rig's table unchanged).
- **Tests:** `tests/fork_9bebf34_tangent_end_cases.rs` and the mixed corner tests in `blend::fillet_corner::p3`.
