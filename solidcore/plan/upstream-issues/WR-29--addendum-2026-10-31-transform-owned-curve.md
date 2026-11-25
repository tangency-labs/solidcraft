Updates: WR-29

#### WR-29 (update, 2026-10-31). `SmProjectedCurve::Transform` permits `OwnsCurve::Yes` (`m_bOwnsCurve == TRUE`) alongside `UNSURE`

The record's defect is in pinned v1.0.1 `source/SMLib/src/SmProjectedCurve.cpp`: every other
mutating operation on `SmProjectedCurve` — `EditParameterization` (`:252`),
`ReverseParameterization` (`:367`) and `Trim` (`:395`) — tests `if (m_bOwnsCurve == 0)`
(`FALSE`), allowing both `TRUE` (`1`, owned base curve, including every curve deserialized by
`SmProjectedCurve::ReadFromDB` at `:990`) and `UNSURE` (`2`, the member initialiser). In
`SmProjectedCurve::Transform` (`:434`), however, upstream tests `if (m_bOwnsCurve < 2)`, which
rejects `TRUE` (`1 < 2`) with `SM_ERR` (`1001`) on any non-identity placement or non-unit scale.

- **What happens (default, pinned):** `ProjectedCurve::transform` (and `Curve::transform` on a
  `CurveKind::Projected`) returns `SM_ERR` unless `owns_curve == OwnsCurve::Unsure`; any
  projected curve constructed with `OwnsCurve::Yes` or read from `.smb` via `ReadFromDB` fails
  to transform under a non-identity `Axis2Placement` or non-unit `scale`.
- **Fix (opt-in):** `Kernel::fixes.projected_curve_transform_when_owned` (`UpstreamFixes`),
  default `false` (pinned behaviour). When `true`, `ProjectedCurve::transform_at_depth` rejects
  only `self.owns_curve == OwnsCurve::No` (`FALSE`), allowing both `OwnsCurve::Yes` and
  `OwnsCurve::Unsure` through both `ProjectedCurve::transform` and `Curve::transform` (including
  nested projected curves). Borrowed base curves (`OwnsCurve::No`) still return `SM_ERR` on
  non-identity transforms, and identity transforms still return `Ok(())` early.
- **Tests:** `wr29_owned_projected_curve_transforms_under_opt_in_all_projection_types`,
  `wr29_borrowed_still_rejected_and_unsure_and_identity_unchanged`, and
  `wr29_read_from_db_projected_curve_transforms_and_flag_lookup` in
  `crates/solidcore/src/curve/projected_curve.rs` (#2156). They verify default `SM_ERR` (`1001`)
  non-mutation parity, opt-in geometric equivariance (points and tangents) across
  `ProjectionType::{Parallel, Perspective, Rotation}` on 3D `Line`, 3D `BSplineCurve` and nested
  `ProjectedCurve` bases under rigid rotation + translation, isotropic scale + rotation, and
  anisotropic scale, `OwnsCurve::No` rejection, bit-identical `OwnsCurve::Unsure` parity, binary
  and ASCII `read_from_db` round-trips, and `UpstreamFixes::flag_mut` registration.
- **Confirmed:** read at the pin (`3fd5b95`, `SmProjectedCurve.cpp:417-468`, `:990`); the pinned
  behaviour is retained by default and no difftest case changes.
