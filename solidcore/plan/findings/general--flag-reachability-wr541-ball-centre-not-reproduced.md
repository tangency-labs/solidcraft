# General: WR-541 `fillet_corner_ball_center_from_axes` is reached by the tray but changes nothing (2026-10-31)

- **Flag:** `Kernel::fixes.fillet_corner_ball_center_from_axes` (WR-541, fork tangency-labs/smlib@bc6ddf9), with its prerequisite `fillet_corner_ball_patch` (WR-420).
- **Audit:** `plan/findings/audits/flag-reachability-2026-10-31/` classifies it as C: no test sets it.
- **Tried:** the record's trigger as the port can build it, the `issue_416_fillet_seam` tray. A box of w x w x 10 has its bottom edges rounded r1, then its vertical edges and the seams between bottom rounds rounded r2: (w, r1, r2) = (25, 2, 2), (25, 3, 3), (25, 2, 3), (31.3, 2.7, 2.7). Each was run with `fillet_corner_ball_patch` on and `fillet_corner_ball_center_from_axes` on and off.
- **Result:**
  - The guard (`FilletSolver::sphere_fillet_geom_parts`) is reached.
  - The volumes agree to about 1e-12 with and without the flag, e.g. 6135.179908641955 and 6135.179908641957 for (25, 2, 2).
  - (25, 2, 3) is 1001 either way.
  - The port's rounds are `SmCone`s (8 Cone faces), and their trace points' centres already agree, so centring the ball where the axes meet moves nothing.
- **Why:** the fork's failure needs rounds that are cylinders with their axes a rounding apart from SMLib's own cones. On SolidCraft's tray (rig fillets/1557) the rounds were read back from its records that way, and the end point's centre missed by 2.8e-4.
- **Waits on:** a tray whose bottom rounds are cylinders a rounding off the cones (the rig part, or a body read from a file written that way). No fork fix is missing in front of the branch.
