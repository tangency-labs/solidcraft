Updates: WR-448

Follow-up to the 2026-10-29 addendum (tangency-labs/solidcore#1490, #1491).

- **Spheres and tori are never inside out either.** With `fixes.revolve_analytic_profile_faces`, `sweep::analytic_revolution` no longer builds a sphere or torus inside out. The fork's `AnalyticSurface` (tangency-labs/smlib 9853e54) builds the sphere over its latitude range and the torus with `CreateCanonical(.., FALSE, FALSE, ..)`. When an arc falls round its circle, its surface is laid out as the B-spline of the reversed arc. The earlier layout gave an inside-out torus whose surface flag the `SmTorus` constructor drops (WR-500), so it failed Test #11.
- **A one-curve profile no longer takes the special case.** 9853e54 deletes `sm_CreateAnalyticRotationalSweep`. With the fix, a whole turn of a one-curve profile now skips `analytic_rotational_sweep` and goes through the same per-curve rule as a longer profile.
  - A whole circle's torus then has X radial into the profile's half-plane, so its seam lies at the profile.
  - Upstream's special case takes X from the circle's plane normal, which puts the seam a quarter turn away.
