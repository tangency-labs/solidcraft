# intersect.surface_drop_lib_p3 (PR #136)

- **The traced paths run since `surface.offset_surface` landed** (merged main,
  2026-10-18): `SetProperties` drops every non-degenerate curve tangent onto the
  `SmOffsetSurface`, so the 652 cases that deferred on it now compare (1310 pass, the
  219 left defer on the `curve.crv_on_surf` dispatch gap). A point curve's
  `GetNextPoint` never succeeds (its `m_eDropDerivRtn` stays unset, so the status is
  `99999999` on both sides), so only a point on a pole gives a curve (the pole line).
- **Two disagreements surfaced by that merge, both fixed.** (1) Harness: the runner
  encoded the record's `m_sFailStatus` as `0` only for a Rust status of `0`, but
  `SM_SUCCESS` is `1000`; the shim's `st` maps `SM_SUCCESS` to `0`, so every success
  record (110 cases: `cpp=0`, `rust=1e3`, on B-spline surfaces) disagreed. (2) Port
  (`intersect.surface_drop_lib_p2`): `SetProperties` ignores the status of
  `rOffset.EvaluateDirectionalDerivs` (`SmSurfaceDropLib.cpp:1871`), which fails at a pole
  of the offset (a cone apex, gen0125, tolerance -1); the port propagated it as `SM_ERR`
  (record status 1001 against 1032). It now leaves `sOffsetPD` at its default
  `SM_UNDEF_DOUBLE` values, the tangent drop tests bad and `SM_ERR_BAD_TANGENT_DROP`
  results, as upstream.
- **`SmLine` between (nearly) coincident points is not a point curve.** The constructor
  replaces a vector shorter than `SM_EFF_ZERO` by `(1, 0, 0)` with scale 1 and an empty
  analytic domain, so over `[0, 1]` it is a unit line. The difftest builds point curves as
  B-splines with coincident control points.
- **`SmCrvOnSurf` over planes and surfaces of revolution** (the final distance check) is a
  `curve.crv_on_surf` dispatch gap (the ported curve holds a B-spline surface only).
- **`SmTol::IsDegenerate(const SmCurve*, ..)` of an edge's curve** (`geom::tol::is_degenerate_curve`,
  `zone_tol_3d_of_curve`): the zone tolerance of a curve owned by an edge is the edge's
  tolerance (`SmEdge::GetTolerance`, `topology.edge`), and `SmCurve::GetEdge` follows the
  owner chain of generated curves and edge uses. A bare `Curve` holds only its owner's
  `ObjectRef` handle, with no B-rep arena to read the edge from, so those cases return
  `Unported`; a curve without owner (or with a non-edge owner) gives the system zone
  tolerance and is compared (`geom.tol` difftest, curve cases).
- **A plane with a zero-width or zero-height natural domain has no NURB** (the
  shim ignores the `NULL` of `GetOrCreateGwNurbPointer`, the difftest builders ignore
  `make_nurb`'s error): the C++ `DropCurve` does not crash on it and agrees with the
  port (fixtures `plane_natural_zero_width`, `plane_natural_zero_height`, WR-100), so the
  `intersect.surface_drop_lib_p2` FFI guard need not reject such planes.
