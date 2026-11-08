# surface.unbend_volume (`SmUnbendVolume`)

All non-deferred difftest cases agree bit for bit with upstream (`max_rel_err = 0`, 1874
default cases, a 30,374-case run clean). The 67 deferred cases are `IsCurveInParamDomainSimple`
(66: `SmCurve::HasPlaneCrossings` needs `SmSurface::GlobalCurveIntersect`, unit
`surface.surface_p8`) and `ReadFromDB` (1: base read `SmVolume::ReadFromDB`, unit
`surface.volume_p2`). No upstream test builds an unbend volume. `SmVolume::EvaluatePoint` and
`SmVolume::IsPointInParamDomain` (unit `surface.volume_p2`) were co-ported here; when
`surface.volume_p2` merged, the co-ports were replaced by its versions
(`Volume::evaluate_point`, `Volume::is_point_in_param_domain`, which also keeps the unbend's own
image when its next map fails, as below) and `ReadFromDB` now reads the base data: 1874 cases,
1808 pass, 66 deferred (`surface.surface_p8`). Upstream behaviour worth knowing (all ported as
written):
- **`IsPointInParamDomainSimple` is always `TRUE`.** The inline header body calls
  `IsOnBoundarySimple(P, bB, bB, bB)` with one `SmBoolean` for all three out-flags; the last
  write `rbOnW = FALSE` clears it, so `return(rbOnU || rbOnV)` is `FALSE` for every point. As a
  result degenerate segments in `IsLineInParamDomainSimple` are always "in", and the domain check
  of `GlobalPointSolveSimple` never fails (it also checks the *projection* point as if it were a
  parameter point). `IsOnBoundarySimple` called with distinct flags works as documented.
- **`GW_MAX_DERIV` is 5** (`SmNurbsCrv.h`), not 3: `Evaluate` / `EvaluateSimple` accept
  `n = 4, 5` and return zero derivatives above the third. With a next map and `n > 3`, the
  `Map` call fails before the 64-entry `aNextDerivs` buffer could overflow.
- **`EvaluateBoundingBoxSimple` maps the box edges through the whole volume.** The lines that
  would make the volume a simple map for the duration are commented out ("do not compile on
  gcc"), so the "ProjSpace" boxes include the orient and next maps. A bounded box with a side
  of zero length makes `SmLine::CreateLineSegment` fail on that edge; upstream then
  dereferences a null `pLine` (first edge) or the line the previous `SmCrvInVolume` already
  deleted (owner flag 1). The port returns `InvalidInput`; the ffi rejects such boxes.
- **`operator==`** overwrites the neutral-distance comparison with the singularity-tolerance
  comparison: unbends differing only in neutral distance compare equal.
- **`IsLineInParamDomainSimple`** tests the end points' discontinuity with
  `SM_ARE_SAME(P.y, m_dSingularityTol)` (against the tolerance, not `0`).
- **Constructor repair** has the same `Unitize` status comparison as the twist volume (only
  the last zero-vector branch runs); a neutral distance below the clamped singularity tolerance
  becomes `1.0`. The default constructor gives neutral distance `0` and tolerance
  `SM_UNDEF_DOUBLE`.
