#### Updates: WR-255 (2026-11-17). The end move takes the control points next to the end with it

- **Change (tangency-labs/solidcraft#14257, WR-794):** on both paths of
  `topology::face::p2::fix_uv_trim_curves` (upstream's stretch and
  `Kernel::fixes.uv_trim_corner_end_points_only`) the move of a trim's end onto the vertex's UV
  point is spread over the control points within 20 gaps of the end along the control polygon
  (`spread_end_move`).
- **What stays:** a trim whose end control leg is at least 20 gaps long is edited as before, bit
  for bit: its end control point moves alone. With the fix on, the knots do not change and the
  control points from the one the move stops at on are untouched
  (`issue_959_fix_uv_trim_curves_keeps_interior_parameters`, gap 1e-3 at an end leg of 2.5).
- **What changes:** with a shorter end leg, "only the end control points move" no longer holds:
  the control points before the stop move by a share of the gap, each by less than the gap.
