# General: WR-547 addendum: the fork's L-box corner diverges (2026-10-31)

Updates: plan/findings/general--flag-reachability-wr547-coons-patch-not-reproduced.md

The fork's capi test (cae8618, `ops_kernel`) rounds only two edges, the notch floor's concave edge and its convex side edge, of a 60 x 40 x 20 box less the notch (40, 20, 10)-(70, 50, 30), at r 2 and r 5. The corner patch at (60, 20, 10) is 38.5 degrees off by default in the port, exactly the fork's value with its hunk reverted, and 0.13 degrees with the flag. Ported as `crates/solidcore/tests/fork_cae8618_l_box_corner_patch.rs` (fails with the flag forced off).
