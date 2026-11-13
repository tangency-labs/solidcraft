# General: WR-455 addendum: the fork's own tests do not diverge on the contact fix alone (2026-10-31)

Updates: plan/findings/general--flag-pin-wr455-tangent-contact-lands-on-the-end.md

With only 51eefa5's `SmCurve.cpp` hunk (`sm_MoveContactsToCurveEnds`) reverted on the fork's replay/1.0.1 (0f7b209), its `ops_kernel` tests pass, including the ladder part 09/011 shell. Their divergence comes from the stitch hunk (`offset_stitch_at_body_tolerance`). In the port, the part shelled with the stitch fix changes by 4e-7 mm3 (3e-11 relative) with the contact fix.
