# General: WR-536 addendum: the fork's own four-disk test diverges in the port (2026-10-31)

Updates: plan/findings/general--flag-pin-wr536-far-disks-cover-already-holds.md

The fork's capi test (67b8a1e, `ops_kernel`: four r 2 disks at (30, 20), (1500, 20), (2970, 980) and (30, 980), joined in pairs and then all four) does diverge in the port. The pairs join cleanly either way, but by default the four-disk join fails with 1006; with the flag it is four faces, every edge tolerance within 1e-4. On the fork (replay/1.0.1 0f7b209 built from source) reverting the hunk gives pair tolerances of 5880 and 11800 and a failed four-disk join. Ported as `crates/solidcore/tests/fork_67b8a1e_four_far_disks.rs` (fails with the flag forced off).
