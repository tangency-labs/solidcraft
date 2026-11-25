# sweep.api_surfaces: review fixes (PR #37, 2026-10-13)

- The FFI guard (`crates/solidcore-ffi/src/units/sweep_api_surfaces.rs`) used to reject
  2D NURB data (`z = NL_NOZ`) and any `dim != 3` for the `Sweep`, `Ruled`, `Skin` and
  `Revolution` curves. Upstream `CreateSweepSurface`, `CreateRuledSurface`,
  `CreateSkinnedSurface` and `SmApiCreateSurfaceRevolution` take these curves without
  crashing (reviewer probe: 4081 generated cases plus explicit 2D fixtures, 0 failures,
  0 crashes). The guard now only requires positive weights (non-positive weights stay
  rejected outside `Extrude`); the sweep, ruled and skin generators produce 2D curves.
  This was lost coverage, not a numerical disagreement: the Rust port already agreed.
- 2D NURB revolution (`adv_rev_2d_nurb`, `adv_rev_dim2_3ddata`) runs through the C++
  without crashing; the Rust side is deferred on `CreateRotationalSweep`
  (`primitive.primitive_creation_p3`), so it is compared once that unit lands.
- `SmApiCreateSurfaceRevolution` returns `SM_ERR` when `SmCurve::Copy` fails (allocation
  failure only); the Rust `BSplineCurve::copy` is infallible, so the port has no such
  path (documented in the module's Deviations).
- `fixtures/sweep.api_surfaces/adversarial.json` (59 cases): sweep vectors at and just
  above `SM_EFF_ZERO`, scales 1e-9..1e9, 1D/2D/rational extrudes, degree 17, rational
  and point-degenerate sweep paths, ruled surfaces across degree/domain, 12-profile and
  mixed-kind skins, coincident profiles, skin-primitive validation (counts of 2^20,
  all-NULL curves, zero profiles), 2D ruled/skin/sweep/revolution. All pass or defer.
