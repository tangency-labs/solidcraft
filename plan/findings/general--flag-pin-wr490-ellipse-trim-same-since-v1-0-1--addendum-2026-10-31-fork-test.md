# General: WR-490 addendum: the fork's replay/1.0.1 dropped the change (2026-10-31)

Updates: plan/findings/general--flag-pin-wr490-ellipse-trim-same-since-v1-0-1.md

The fork's replay/1.0.1 branch (0f7b209, the line SolidCraft pins) has no seam-line change in `sm_GetEllipseTrimIntervals`. It runs v1.0.1's code, and its `equal_rods_join` / `equal_tubes_join` tests pass on it. The flag has no fork counterpart left to test against.
