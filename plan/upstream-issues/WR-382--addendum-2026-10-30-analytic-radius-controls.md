# WR-382: analytic radius and healthy-curve controls

Updates: WR-382

PR #677 retains the analytic quadratic `C(t) = (w(t-1/2), (t-1/2)^2, 0)`
with `w = 0.001`, minimum radius `5e-7` and nonzero speed at the minimum.
Tests exercise the existing default-off `Kernel::fixes.bspline_check_min_radius`
switch in 2D/3D, both base-check modes and calls without a report list.
The default accepts the curve as pinned C++ does; the enabled mode reports test
13/14 with the analytic radius and parameter. Healthy parabolas, lines and a C1
straight-to-bend control remain valid when enabled. Nine native fixtures retain
the same inputs under the default policy.

No production algorithm changes or additional solver dependency are included.
The former seven issue-specific mismatch exemptions are removed: ordinary native
comparisons use the default policy. Main's unit status and independent records
are preserved. Historical unconditional-fix observations remain in the per-entry
finding `nurbs.b_spline_curve_p3--issue-475-curvature-radius-review-history.md`;
they do not describe default behavior of this revised PR.
