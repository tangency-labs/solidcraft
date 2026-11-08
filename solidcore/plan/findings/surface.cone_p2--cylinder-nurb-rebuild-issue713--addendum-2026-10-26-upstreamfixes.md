# surface.cone_p2: adopt main's opt-in cylinder rebuild (#724, 2026-10-26)

Updates: plan/findings/surface.cone_p2--cylinder-nurb-rebuild-issue713.md

PR: https://github.com/tangency-labs/solidcore/pull/724

Main commit `0b6618865` independently added the correction for #713 under
`Kernel::fixes.cylinder_rebuild_mean_radius` and recorded the defect as WR-385.
The PR now builds on that existing flag and record. The previous 52 differences
and blocked state in the historical finding describe the original unconditional
implementation; they do not describe the revised default behavior.

With the flag off (the default), the strict `EFF_ZERO` equality test and retained
semi-angle match pinned C++ `c9c5979`. With it on, accepted cylinders use the mean
of the recovered radii within the existing strict `EFF_ZERO_SQRT` bound and their
semi-angle is explicitly set to zero. A larger taper is rejected before analytic
fields change. Ordinary cones retain the same angle and radius with either flag.

The residual bug in main's opt-in is independently reproduced by
`rebuild_cylinder_small_arc_clears_a_stale_semi_angle`: a radius-9 cylinder,
height 3, sweep `360 / 256` degrees, carrying angle
`-4.3550699142343645e-10` degrees from an earlier buggy rebuild. Main equalizes
the radii but keeps that angle because the later angle update is conditional on
unequal radii. The regression fails against main's implementation and passes
with the explicit reset. The same stale angle is retained with the flag off.
Both UV orders and orientations are tested.

The 24 issue713 differential fixtures run against the unpatched C++ reference
with the flag off. A separate runner test exercises all 24 with the flag off and
on, including eight rejected tapers on the enabled path; library tests add
multiple scales, shallow cones and the small-arc stale-angle case. No tolerance,
absolute floor, known mismatch or FFI guard is loosened. No SMLib 1.0.1 upgrade
or reference backport is required; `tangency-labs/smlib@f06b48a` is the related
fork correction cited by #713, not a build dependency.
