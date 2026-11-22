# surface.surf_of_revolution_p1 (2026-10-13)

Ported `SmSurfOfRevolution` part 1 (`crates/solidcore/src/surface/surf_of_revolution/p1.rs`)
with `CreateCanonical`, `MakeNurb` and `MakeNurbWithSweepParams` co-ported (units `_p2`,
`_p3`). difftest: 807 cases (207 fixtures, 600 seeded), 776 bit-identical, 0 failed,
31 deferred (`IntersectWithPlane` of a planar generator needs `SmPlane`,
`surface.plane_p1`); `--perturb` detected 776/776.

- **`IntersectWithPlane` after `surface.plane_p1` (merge of main, 2026-10-13).** The
  `UNPORTED(surface.plane_p1)` stub (`SmPlane::GetPosition`) is wired: the method takes a
  real `Plane` and the difftest builds it with `Plane::create_canonical`, as the shim. The
  31 planar-generator cases stay deferred, now on `surface.surface_p1`: before any branch
  upstream takes `crPlane.CalculateBoundingBox(GetNaturalUVDomain(), NULL, &sPlanePBox,
  NULL)` with the default `bExpandPosBoxesByZoneTol3d = TRUE`, which expands the pseudo
  box by `GetFace() ? GetFace()->GetTolerance() : ...` (`SmSurface::GetFace`, unported;
  upstream ignores the status, `Unported` still propagates). The body below the plane box
  (perpendicular / parallel axis cases) is not ported: both of its curve-producing paths
  call `SmPlane::TrimCurveToPlaneDomain` (`surface.plane_p2`), so it stays behind an
  `UNPORTED(surface.plane_p2)` stub for that unit's porter, unreachable until `GetFace` is.

- **Generator owner stubs not wired.** `SmLine`, `SmEllipse`, `SmHyperbola` and
  `SmParabola::MakeNurb` reparameterise a generator owned by a surface of revolution to
  `((SmSurfOfRevolution*)m_pOwner)->GetGenDirParamExtent()`. The Rust curves only hold an
  `ObjectRef` (type code + key) for their owner, which cannot reach the surface, so the
  four `UNPORTED(surface.surf_of_revolution_p1)` stubs stay (they return `Unported` for
  such an owner). `SurfOfRevolution::gen_dir_param_extent` is ported; wiring needs an owner
  model that can hand a curve its owning surface's data. The Rust `SurfOfRevolution` leaves
  its generator unowned, so it never reaches the stubs; upstream's owner-driven step only
  differs when `MakeNurbWithSweepParams` adjusts a generator whose STEP interval differs
  from the surface's V domain (never through `CreateCanonical`).
- **Upstream re-entrancy hazard (avoided by the harness).** The constructor sets the
  generator's owner, then evaluates it. An analytic generator without NURB (an `SmLine`
  fresh from its two-point constructor) then runs `MakeNurb`, which asks the owner for
  `GetGenDirParamExtent`, whose `GetNaturalUVDomain` makes the surface's NURB through the
  virtual `MakeNurb` from inside the constructor. The difftest shim makes the line's NURB
  first; the Rust port evaluates the generator without the owner detour.
- `RebuildPolarConverter` dereferences a null iso curve when `CreateIsoParametricCurve`
  fails (upstream only asserts); the port returns the error.
- `sm_TestForIsoCircles` leaves outputs unwritten when no iso curve is non-degenerate or no
  two iso planes intersect; with parallel planes after the first common line, it keeps
  comparing the last line found (stale `smgu_IntersectTwoPlanes` outputs). The port returns
  the unwritten outputs as `None` and keeps the stale-line behaviour.
- **Review fixes (PR #15).** `CreateSurfOfRevolution` reparametrises the sweep circle with
  arc length when the generator is an `SmLine` parallel to the axis within 0.001 degrees
  (a cylinder: sweep knots span the arc length, `0..2*pi` for radius 1, not `0..1`); the
  port missed it because the generator reached `BSplineSurface::create_surf_of_revolution`
  as its plain B-spline base. `make_nurb_with_sweep_params` now passes the line vector to
  `create_surf_of_revolution_with_line_vector`. `CreateCanonical` ignores the status of
  `Unitize`, so a zero axis (length squared below `SM_EFF_ZERO_SQ`) becomes `(0, 0, 1)`;
  the port does the same (`unitize_or_z`). `SetOutOfBoundsEnabled` also sets the
  generator's flag, as upstream's override; the shim now calls it instead of writing the
  base flag. The FFI accepts any finite `CreateCanonical` axis, and only unit placement
  axes for the constructor: a Y axis with `|y|^2 - 1 > SM_EFF_ZERO` makes
  `SmBSplineCurve::IsOnPlane` (via `smgu_PlanePointDistance`) fail before writing
  `bOnPlane`, which the constructor then reads uninitialised for a planar non-line
  generator. A zero sweep (empty angular domain) stays rejected by the FFI guard.
