# General: WR-490 `ellipse_trim_seam_lines_span_cone_height` gives the default's result since v1.0.1 (2026-10-31)

- **Flag:** `Kernel::fixes.ellipse_trim_seam_lines_span_cone_height` (WR-490).
- **Audit:** class B. `surface::cone::p2::tests::seam_crossing_below_a_trimmed_domain_keeps_the_ellipse` reaches the guard; nothing fails forced off.
- **Why:** `plan/upstream-issues/WR-490--addendum-2026-10-27-changed-in-v1-0-1.md`: v1.0.1's `sm_GetEllipseTrimIntervals` already takes the trim circles at the domain's v limits and classifies with `sm_IsConePointInTrimDomain`, so on the record's unit case the default and the flag both return the 263.62-degree interval. The audit's all-on SolidCraft cases that reach the guard (788-equal-bores-crossing, catics-3d18-h01) give identical output with the flag forced off.
- **Waits on:** the record's boolean triggers (the equal rods, the steel-026 tubes), which the addendum notes were not rechecked against 1.0.1. If they agree by default too, the flag can be retired.
