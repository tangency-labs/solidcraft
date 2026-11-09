# curve.line (`SmLine`)

Difftest: one program per case (a line construction, then up to ~6 ops on it, then the
line's full state: analytic data, B-spline part, natural interval), so state changes made
by one op (lazy NURBs, trims, joins, reparameterisations) are checked by the next.
Upstream behaviour worth knowing:

- **Lazy NURB.** The two-point constructor makes no NURB; `GetNaturalInterval` then
  returns the analytic domain and `Length` uses `interval * scale` instead of the end
  point distance. Inherited `const` queries (`GetEnds`, `GetNumberControlPoints` inside
  `CreateAnalyticCurve`, ...) make the NURB through `GetOrCreateGwNurbPointer` (a
  `const_cast` + virtual `SmLine::MakeNurb`), so a later `Length` changes branch. The port's
  `Line::ends` / `Line::create_analytic_curve` take `&mut self` and do the same; the
  read-only variants make it on a copy.
- **`SmBSplineCurve::DeformToEndPointTargets` ignores `DegreeElevate`'s status**
  (`SmBSplineCurveNLib.cpp:3038`). The port of `nurbs.b_spline_curve_n_lib` propagated it;
  fixed in this unit (its difftest stays green, 2282 cases). For a NURB over a zero-length
  knot interval (a degenerate line: `[a, a, a, a]`) `N_CrvElevateDegree` fails
  (`N_KnotVectorIsValid`), upstream then runs the degree-3 indexing scheme on the degree-1
  curve and reads past its arrays (undefined behaviour; it happened to "succeed" in our
  runs). The port returns `InvalidInput` ("knot index out of range"). The curve.line
  generator only deforms regular lines into a new B-spline (first op only).
- **Short database streams.** `SmLine::ReadFromDB` / `SmBSplineCurve::ReadFromDB` on a
  truncated binary stream throw (`SmDatabaseIOStream`) or read uninitialised values, and a
  short ASCII header leaves `n`, `p` uninitialised before the allocation: not difftested;
  the ffi only builds complete streams.
- `IsLine` always answers `TRUE`; `IsClosed` / `IsArc` `FALSE`; `IntersectWithLine`
  sorts a coincident-range solution by this line's parameter, re-evaluating the start with
  the second point's parameters. `DropPointFast` compares the clamped and raw NURB
  parameters with `!=` (a NaN parameter recomputes the distance).
