# General: WR-455 `curve_contacts_at_curve_ends`: no input found whose tangent contact lands inside the stretch (2026-10-31)

- **Flag:** `Kernel::fixes.curve_contacts_at_curve_ends` (WR-455 part 2, fork tangency-labs/smlib@601d804).
- **Audit:** class B. `issue_855_offset_stitch_keeps_the_bodys_tolerance` sets it alongside `offset_stitch_at_body_tolerance`; forcing it off changes nothing. The record already says no small input was found whose point solution lands inside the tangent stretch.
- **Tried (origin/main 36f1a9a67, instrumented):** `Curve::global_curve_intersect` of a line along x (from x = 0, and from x = -5 so that only the other curve's end can apply) with a cubic Bezier tangent to it at its start (0, 0, 0), curving away as y ~ 3 c t^2 with c = 1e-2, 1e-3, 1e-4, at tolerances 1e-6 and 1e-5, with the flag on and off.
- **Result:** every case gives one single-value solution at the cubic's start (t = 0) both ways. The solver already lands on the end, so `move_contacts_to_curve_ends` has nothing to move. In the all-on SolidCraft cases, forcing the flag off moves 5797/dia_30's volume by 1e-15 relative only.
- **Waits on:** SolidCraft's ladder parts 09/011, 10/008 or 11/004 (a round whose offset is tangent to the wall's offset), where the contact was found 3.3e-4 of parameter inside the curve's start. No fork fix is missing in front of the branch.
