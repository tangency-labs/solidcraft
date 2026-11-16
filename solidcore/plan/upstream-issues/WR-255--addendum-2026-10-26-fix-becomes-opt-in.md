#### Updates: WR-255 (2026-10-26). The fix becomes opt-in; the default is upstream's

- **Change (decision on tangency-labs/solidcore#959, following #1077):** the port's
  `topology::face::p2::fix_uv_trim_curves` now runs upstream `c9c5979`'s drop / `Trim` /
  `EditParameterization` stretch by default. The fork's fix (tangency-labs/smlib@5af7070:
  only the end control points move) is the opt-in
  `Kernel::fixes.uv_trim_corner_end_points_only` (`UpstreamFixes`, off by default).
- **Replaces** the `Context::trim_uv_curves_at_corner_drops` toggle (removed), whose
  default was the fork fix; the `topology.brep_p10` runner no longer sets it.
- **Difftest:** the three WR-255 known mismatches are removed and pass at 1e-9 by
  default: `topology.brep_p3` `nurbs_off_Mouse_0.1` and `nurbs_off_Mouse_-0.05` (the four
  `SM_NO_POST_EDIT` tracker events of the trim and stretch) and
  `offset.offset_geometry_creation` `gen0350` (the sphere face's 5-point UV trim). The
  Mouse cases also needed the `SM_NO_PRE_EDIT` / `SM_NO_POST_EDIT` that upstream's `Trim`
  and `EditParameterization` send when they do work; `fix_uv_trim_curves_notified` now
  sends them. `topology.brep_p3` and `offset.offset_geometry_creation` become green.
- **Tests:** `issue_959_fix_uv_trim_curves_keeps_interior_parameters` (fix on: same knots,
  interior control points untouched) and `fix_uv_trim_curves_stretches_the_trim_by_default`
  (fix off: the interior knots slide), in `crates/solidcore/src/topology/face/p2.rs`.
- **Callers** that relied on the previous default (e.g. SolidCraft, #570) must set the
  field.
