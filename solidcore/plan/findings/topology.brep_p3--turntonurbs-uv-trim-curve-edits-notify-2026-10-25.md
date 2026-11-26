# topology.brep_p3: TurnToNURBS UV trim curve edits notify (2026-10-25)

- **What.** Four of the six `TurnToNURBS` known mismatches (`gen0138`, `gen0164`,
  `gen0241`, `nurbs_off_toruswvy_0.1`) now pass. Their missing tracker events were all
  `SM_NO_POST_EDIT`s of UV trim curves from `SmBSplineCurve::EditEndPoint` in
  `sm_FixUVTrimCurves` (`SmBrep::ReplaceSurface` -> `SmFace::CreateUVTrimCurves`).
  The curves are owned by edge uses, so the tracker's owner walk reaches their edges.
- **How found.** A temporary backtrace in the shim's notify recorder showed every
  geometry `PRE_EDIT` / `POST_EDIT` in the six cases. Each one is listed with its call
  stack and the owner chain the curve has at that moment:
  - `SmBSplineCurve::EditEndPoint` <- `sm_FixUVTrimCurves`, owner edge use -> edge.
    6 pairs in the gen cases, 2 in `toruswvy`, 10 in each Mouse case. **Ported.**
  - `SmBSplineCurve::EditParameterization` <- `SmSurface::DropIsoCurve` <- `DropCurve`
    <- `SmEdgeuse::CreateUVTrimCurve`, 4 pairs per analytic face. The curve has no owner
    yet, so the tracker records nothing. **Not ported** (no runner observes it).
  - `SmBSplineCurve::Trim` and `SmBSplineCurve::EditParameterization` <-
    `sm_FixUVTrimCurves`, 2 pairs each in `nurbs_off_Mouse_0.1` and
    `nurbs_off_Mouse_-0.05`. **Not ported**: the port skips the drop/trim/stretch step
    on purpose (WR-255, tangency-labs/solidcore#570). The C++ reference still runs it.
    So these two cases do reach the removed branch, contrary to the WR-255 entry's "no
    difftest case reaches" note. Only the tracker shows it, because UV trim curves are
    not exported.
- **Port.**
  - `face::p2::fix_uv_trim_curves_notified` (through
    `face::p3::create_uv_trim_curves_notified`, called by `brep::p2::replace_surface`
    with its environment) brackets each end point move with `SM_NO_PRE_EDIT` /
    `SM_NO_POST_EDIT`: caller and data1 are the curve, under its kernel-issued identity
    (`Curve::notify_key`); data2 and data3 are null, as `SM_NO_GET_OWNER` is.
  - An upstream callback reads the curve's `GetOwner()` and the edge use's `GetEdge()`
    through its pointers. Here the edit reports those two links first, through the new
    `NotifyCallback::owner_link` (`geom::object::report_owner_link`). The change
    tracker stores them in its owner map. UV trim curve owner changes notify nothing
    upstream (`SmCurve::SetOwner` reports 3D curves only), so no event could carry them.
- **Still known mismatches.** `nurbs_off_Mouse_0.1` and `nurbs_off_Mouse_-0.05`:
  - C++ has 34 events, Rust 30. The missing ones are the `Trim` and
    `EditParameterization` post-edits of two curves (`-30 2` and `-30 6`, two each).
  - The lists and every other event agree.
  - Fixing this means undoing the deliberate WR-255 deviation, or patching the
    reference.
- **Remaining upstream geometry edit sites without notifications in the port**
  (`Notify(SM_NO_PRE_EDIT, ..)`; none reached by a tracking runner's cases):
  - `SmBSplineCurve`: `AdjustSTEPInterval`, `DegreeElevate`, `EditEndPoint` (outside
    `sm_FixUVTrimCurves`), `EditParameterization`, `FixRepeatedEndControlPoints`,
    `FixupKnotVector`, `GetControlPolygon`, `MatchWeights`, `RemoveExtraKnots`,
    `ReverseParameterization`, `SetCanonical`, `SetControlPoint`, `SetExpert`,
    `SetFromGwNurb`, `Transform`.
  - `SmBSplineCurveNLib.cpp`: `DampenKnotSpacing`, `DeformToEndPointTargets`,
    `DegreeReduction`, `InsertKnots`, `InsertOneKnot`, `MakeCurvesCompatible`,
    `MakeNonRational`, `Multiply`, `MultiplyLinear`, `Refine`, `RefineCurve`,
    `RemoveKnots`, `RemoveOneKnot`, `ReparametrizeWithArcLength`, `Scale`,
    `ScaleKnotVector`, `SetIntersectionCurveThroughPoint`, `Trim`.
  - `SmLine`: `AdjustSTEPInterval`, `JoinWith`, `Transform`, `Trim`.
  - `SmEllipse`: `AdjustSTEPInterval`, `JoinWith`.
  - `SmParabola` and `SmHyperbola`: `AdjustSTEPInterval`, `ReverseParameterization`.
  - `SmCrvOnSurf`: `EditParameterization`, `ReverseParameterization`, `Transform`,
    `Trim`.
  - `SmOffsetCurve`: `ReverseParameterization`, `SetOffsetDistance`, `Trim`.
  - `SmProjectedCurve`: `EditParameterization`, `ReverseParameterization`,
    `SetCanonical`, `Transform`, `Trim`.
  - `SmCrvInVolume`: `EditParameterization`, `ReverseParameterization`,
    `SetBaseCurve`, `SetVolume`, `Transform`, `Trim`.
  - `SmBSplineSurface`: `FixRepeatedEndControlPoints`, `GetControlPointNet`,
    `GetControlPointsPointer`, `MakeSingular`, `SetCanonical`, `SetControlPoint`,
    `SetExpert`, `SetFromGwNurb`, `Transform`.
  - `SmBSplineSurfaceNLib.cpp`: `ExpandBoundarySpan`, `Reparameterize`,
    `ReparametrizeWithArcLength`, `Reverse`, `TrimWithDomain`.
  - `SmPlane`, `SmSurfOfRevolution` and `SmSurfOfExtrusion`: `Reparameterize`,
    `TrimWithDomain`.
  - `SmSrfInVolume`: `Reverse`, `SwapUV` (already modelled by the brep_p3 `Orient`
    runner's trailer), `Transform`, `TrimWithDomain`.
  - A future port of any of these should reuse `Curve::notify_identity_slot` /
    `Surface::notify_identity_slot` for the caller identity, and
    `report_owner_link` wherever a tracker's owner walk needs a link no
    `SM_NO_CHANGE_OWNER` announces.
