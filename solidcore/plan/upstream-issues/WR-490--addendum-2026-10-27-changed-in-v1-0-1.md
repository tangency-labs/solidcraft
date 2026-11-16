Updates: WR-490

#### WR-490 (update, 2026-10-27). Changed in v1.0.1; the unit case no longer drops the ellipse

The record says NVIDIA 1.0.1 has the same code. It does not: in v1.0.1 (3fd5b95) `sm_GetEllipseTrimIntervals` takes the bottom and top trim circles at the v limits of the given domain, not the cone's own height, and classifies the pieces with the new file static `sm_IsConePointInTrimDomain` (the STEP inversion plus the domain's v and sweep intervals). The seam lines are still evaluated on the domain's v range.

On the record's unit case (a cylinder r 10, h 100 trimmed to v >= 30, an ellipse through (0, 0, 50) whose only seam crossing is at z = 20), the C++ 1.0.1 returns the part of the ellipse inside the domain: one interval of 263.62 deg (`360 - 2 acos(2/3)`), not "outside". The Rust default agrees with it in difftest (`surface.cone_p2` fixtures `wr490_ellipse_on_cylinder_trimmed_v30` and `wr490_ellipse_on_cylinder_natural`). With `Kernel::fixes.ellipse_trim_seam_lines_span_cone_height` set, the Rust result is the same; the tests in `crates/solidcore/src/surface/cone/p2.rs` now expect this for both settings.

Not rechecked: the record's boolean triggers (the equal rods and the steel-026 tubes) against the C++ 1.0.1.
