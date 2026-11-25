# curve.ellipse_p1 (`SmEllipse` / `SmCircle` / `SmPolarConversion`, part 1)

- **`ConvertTo2D` uses `dThetaXRad` for both new axes** when the projected axes are no
  longer perpendicular (`sNewY = R1 cos(ThetaX) X + R2 sin(ThetaX) Y`, a copy of the
  `sNewX` line): the projected ellipse gets equal axes and `SetCanonical` of two parallel
  vectors (its status ignored). Ported as written; `dThetaYRad` is computed and unused.
- **`SmCircle::CreateSimpleOffset`** tests `IsParallelTo(+Z)` first, and `IsParallelTo`
  accepts anti-parallel vectors, so the `-Z` branch (radius minus distance) is dead: an
  offset with the anti-parallel normal grows the radius too. Ported as written.
- **Uninitialised reads upstream.** `SmCircle(P0, P1, P2, ..)` ignores the status of
  `smgu_CircleFrom3Points` (collinear points leave its outputs uninitialised; the shim
  checks the status and does not build); `AssertValid` / `Trim` use the STEP/NURB
  conversion result even when it failed; `ConvertTo2D` reads `adSols[1]` when the
  quadratic has fewer than two roots; `SmPolarConversion::ReadFromDB` reads uninitialised
  locals for unreadable ASCII headers, and `ConvertToNURBSParameter` /
  `ConvertToPolarParameter` / `IsCurrent` index empty or short tables (a record whose
  tables are empty crashes `AdjustSTEPInterval`; the generator only writes tables as
  `SetUpPolarConversion` builds them). The port returns `InvalidInput` in all these cases.
- **Kernel threading.** Joining `SmEllipse` to `Curve` (`CurveKind::Ellipse`) needs the
  kernel's zone tolerance in every override that rebuilds the polar converter
  (`STEPInversion` of the seam): `Curve::{copy, convert_to_2d, convert_t_from_nurbs_to_step,
  step_inversion, edit_parameterization, reverse_parameterization, transform,
  read_from_db, create_analytic_curve, approximate_curve}`, `CurveCache::analytic_curve`,
  `analytic_curve_intersect`, `fix_chain_curves_list`, `crvs_fix_gaps`,
  `BSplineCurve::{create_analytic_curve, create_simple_offset}`,
  `Curve::{calculate_bounding_box, is_planar, approximate_curve_base}`, the curve-on-surface
  methods that copy, reparameterise, approximate or read their UV curve, and the projected /
  offset / in-volume / tangent-field readers and wrappers now take `&Kernel`.
