# WR-55: flat-corner confirmation (2026-10-25, PR #684)
Updates: WR-55

- **Confirmed:** yes; five new flat-corner fixtures and eight
  existing `surface.surface_p7` generated cases return mask zero when optional
  points are requested. Calling without points retains the classified bits.
  Later points remain undefined even when their corners are flat. This confirms
  and supersedes the old unconfirmed/reproduced-by-Rust note under WR-55.

- **Rust port:** preserves the corner mask and evaluates every flagged point.
- **Evidence:** [#525 findings](../findings/general--525-flat-corners-perspective-bounds.md), PR #684.

Main records the corner defect as WR-384 and fixes it behind
`Kernel::fixes.flat_corners_test_bits`. PR #684 retains that default-off policy
and adds direct opt-in tests and default-policy native fixtures.
