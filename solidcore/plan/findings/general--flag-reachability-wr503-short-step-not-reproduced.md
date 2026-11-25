# General: WR-503 `fillet_trace_rejects_short_step`: the tapered-round plate does not take the short step in the port (2026-10-31)

- **Flag:** `Kernel::fixes.fillet_trace_rejects_short_step` (WR-503; SolidCraft oracle design 80, `tapered_round_keeps_the_plate`).
- **Audit:** `plan/findings/audits/flag-reachability-2026-10-31/` classifies it as C: no solidcore test sets it.
- **Tried:** the record's plate, polygon (0,0) (40,0) (40,2) (0,20), 15 high. It is built as a `create_box` 40 x 20 x 15 less a box turned about z through (40, 2), so its planes have UVs in mm as box planes do. Its front top edge got a degree-3 radius law of 8 equal spans with control radii 3 (x9), 2.8, 1.8 (`FilletSession::prepare_variable_radius_edges`). Also tried with a last-but-one radius of 3 and with the plate scaled by 2. Each case ran with the flag on and off.
- **Result:** the guard (`FilletIntersector::compute_next_point`) is reached. Every case gives the rounded plate (7 faces; 6525.045210 mm3 for 2.8, 6524.397730 for 3.0, 52200.141275 scaled), identical with and without the flag. The record's failure (a 5-face strip of 74 mm3) does not occur, so no test can fail with the flag forced off.
- **Why:** the record says only SolidCraft's imported plate (its records' analytic planes) takes the far-short step. Scaled or `extrude`d plates give the rounded plate, as here.
- **Waits on:** the imported plate (SolidCraft's record of oracle design 80) as a fixture file. No fork fix is missing in front of the branch.
