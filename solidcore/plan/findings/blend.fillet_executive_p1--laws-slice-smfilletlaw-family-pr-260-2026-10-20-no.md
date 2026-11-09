# blend.fillet_executive_p1, laws slice (`SmFilletLaw` family; PR #260, 2026-10-20; no disagreements)

- **Corpus.** 51 cases (35 fixture, 16 generated): linear laws from `smFilletTest.cpp`
  (2 -> 0, 2 -> 0.5), the cubic Bezier law r, 1.2r, 0.8r, r over a box edge, sub-maps
  of a two-edge sequence, zero-length maps and intervals, snapping within the scaled
  zero, extension below and above the domain with reuse and regrowth of the cached
  extension, rational and repeated-knot laws, constant (zero-tangent) laws, huge and
  tiny radii and knot scales; after the PR #260 review also zero-tangent apexes
  (quadratic, cubic), reversed maps and intervals, edge maps just below and above
  `SM_EFF_ZERO`, a parameter exactly on the snap boundary and negative radii: 51 pass at
  max_rel_err 0; 500 seeds 535 pass; `--perturb` SENSITIVE 51/51; `--fuzz 2000` FUZZ-PASS.
- **Not compared.** Degree-0 (step) law curves: the shared FFI curve validator of
  blend.fillet_standard_solver_p1 rejects them (interior knot multiplicity above the
  degree), and nothing shows NLib's extension handles them.
- **G1-closed law curve.** For a parameter beyond the domain of a G1-closed law curve
  (the `smFilletTest.cpp` Bezier is one) `CreateExtendedCurve` returns an unextended
  copy (NLib treats such a curve as periodic) and the evaluation clamps; not a crash.
  Both sides agree (fixture `bspline_cubic_bezier_outside_domain_closed_law_copied`).
- **Design.** `FilletLaw::evaluate` takes `&mut self` (upstream caches the extension
  through a const cast); the `SmLinearFilletLaw` / `SmBSplineFilletLaw` constructors
  are `blend.fillet_executive_p2`'s (PR #267). The harness builds linear laws with
  `FilletLaw::linear`; `SmBSplineFilletLaw(pCurve, crEdgeMap)` is not ported, so for
  B-spline laws it mirrors that constructor's zero-length-edge-map fallback.
- Wiring the law into `intersect.surface_intersector_p1` (`EvaluateLawPoint`, opaque
  `L` law parameter) is left for the executive: the call right after it
  (`SmOffsetSurface::SetOffsetDistance` through a shared surface) is still a stub there.
