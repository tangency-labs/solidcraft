## CreateLinearSweep: negative full-circle distance does not terminate (PR #213 review, 2026-10-19)

- Location: `source/SMLib/src/SmPrimitiveCreation.cpp:1290-1360`, the full-circle
  analytic-cylinder branch at `SmCone::AdjustSTEPUVDomain`, before face construction.
- Trigger: one unit circular B-spline or analytic SmCircle in XY, sweep direction +Z, distance -1,
  with or without end caps. Positive and zero distances finish; negative line,
  skew-line and open-arc sweeps also finish.
- Found by new PR #213 review fixtures `linear_circle_cap{0,1}_distance-1`; the analytic generator family adds two further rejected witnesses.
  Confirmed by the native shim running the included upstream implementation:
  both isolated cases exceeded the 10/5-second limits; the combined run ran
  for over a minute without completing. An interrupted GDB run locates the loop
  in `N_CreateEllipticalArcAngTypeTol` via `SmEllipse::MakeNurb`,
  `SmCone::CreateIsoParametricCurve`, `SmSurfOfRevolution::RebuildPolarConverter`,
  `Reparameterize` and `SmCone::AdjustSTEPUVDomain`. Rust reaches its face-construction
  dependency, so these are rejected native fixtures, not parity comparisons.
- The shim preflight repeats the upstream full-circle angular/tolerance and
  axis-parallel tests and rejects only negative-distance inputs selecting that
  cylinder branch. No kernel tolerance is changed and negative line/open-arc
  fixtures remain accepted.
