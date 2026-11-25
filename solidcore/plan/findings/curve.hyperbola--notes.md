# curve.hyperbola

- `SmHyperbola::MakeNurb` fails (`SM_ERR`, no NURB) for a degenerate analytic domain
  (parallel end tangents in `smgu_LineLineClosestPoint`) or when the middle tangent
  line cuts the chord outside `(SM_EFF_ZERO, 1 - SM_EFF_ZERO)`; the constructor ignores
  the status. `GlobalPointSolveSTEP` on such a curve reaches `SmCurve::GlobalPointSolve`,
  whose curve cache dereferences the missing NURB (`SmBSplineCurve::EvaluatePoint`):
  upstream segfaults. The difftest shim refuses those inputs and the generator avoids
  them; the Rust side returns `Unported` there anyway until `curve.curve_p1` lands,
  whose porter must decide what a NURB-less curve does (`SM_ERR` is the natural port).
- Upstream frees the NURB in `ReverseParameterization` and rebuilds it lazily through the
  virtual `MakeNurb` in `GetOrCreateGwNurbPointer`. The Rust B-spline base cannot reach
  the override, so `Hyperbola::reverse_parameterization` rebuilds it at once (same NURB);
  the difftest shim calls `GetOrCreateGwNurbPointer` before reading the state.
- `MakeNurb` of a generator curve of `SmSurfOfExtrusion` / `SmSurfOfRevolution` (owner
  type) returns `Unported` (`surface.surf_of_extrusion_p1`, `surface.surf_of_revolution_p1`);
  the hyperbola methods that make the NURB take the owner as an argument.
