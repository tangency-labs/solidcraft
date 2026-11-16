# General: WR-537 addendum: with the fork's seam angle the hair-off pin diverges (2026-10-31)

Updates: plan/findings/general--flag-pin-wr537-hair-off-pin-default-already-empty.md

The fork's capi test (577f7be, `booleans` `hair_off_pin_at_a_seam`) extrudes both cylinders from a circle whose seam is at 0.6131 rad, on a 20 x 20 x 5 plate. With that geometry the port's default fails with 1001, as the pinned C++ 1.0.1 does, and with the flag the intersection is empty, as the fork's test requires. The earlier tries used `create_cylinder` (seam at 0). On the fork, reverting the hunk turns SMLIB_ERR_EMPTY into 1001. Ported as `crates/solidcore/tests/fork_577f7be_hair_off_pin_seam.rs` (fails with the flag forced off). The fork test's 0.01 mm case, a crescent, needs another fork change: the pinned C++ and the port both fail it.
