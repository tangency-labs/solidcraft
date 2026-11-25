# curve.curve_p2 (SmCurve part 2: the curve base class)

No Rust-vs-C++ disagreement: 3012 of 3012 non-deferred difftest cases bit-identical (18
fixtures re-expressing the `prog_test` / `SM_API_test` curves — the `CreateLineSegment`
line and `CreateCircleSegment` circle of `smCurveTest.cpp`, `ApproximateLength(ivl, 20)` of
`TestSmPrimitives`, the circle seam continuity — plus 3000 generated over B-spline,
Hermite and arc curves, the static helpers and the solver box tests); `--perturb` is
SENSITIVE. 6 cases are deferred: `CreateAnalyticCurve` of B-splines reaching
`SmCurve::IsArc` (`curve.curve_p5`) or building an `SmLine` (`curve.line`).

The port introduces `curve::curve::Curve` (the `SmCurve` object: a `CurveKind` enum over
the ported curve classes plus the owner) and `CurveEvaluate` (the evaluation virtuals the
base numerics call, implemented by every kind and by `Curve`). New curve units add a
`CurveKind` variant and fill its arm in each `match` of `curve/curve/p2.rs`; base
numerics written over `CurveEvaluate` work for them unchanged.

Wiring its stubs made `curve.hermite_curve` (93 -> 0 deferred) and
`nurbs.b_spline_curve_n_lib` (57 -> 0) green and dropped `nurbs.b_spline_curve_p1` from 60
to 4 (`curve.curve_p1`), `nurbs.b_spline_curve_p2` from 317 to 150 (`curve.curve_p4`,
`curve.curve_p5`, `curve.line`) and `nurbs.b_spline_curve_p3` from 391 to 359 (no
`curve.curve_p2` cases left; the rest now reach `curve.curve_p4`/`_p5`/`_p7`/`_p8`). For
p3's `AssertValid`, `SmTol::IsDegenerate(this)` (the object dispatch `geom.tol` does not
port) is computed in place for an edge-less curve — `IsDegenerate` at
`GetXSectTol3d(zone, zone)` with the system zone tolerance — so
`BSplineCurve::assert_valid` now takes the `Kernel`. Stubs **not** wired: the `UNPORTED(curve.curve_p2)` sites
in `geom/tools.rs`, `geom/solution_array.rs` and the `geom/tol.rs` comment call `SmCurve`
methods on opaque handles (`C: ?Sized`, `O: SolutionObject`) that cannot yet be turned into
a `Curve` (in the difftests those objects are `SmLine`s / edges of B-rep files). They keep
their markers and still report `curve.curve_p2` as the blocker
(`geom.tools` 68, `geom.solution_array` 16, `geom.global_solver` 9 cases); whoever gives
solution objects and edges real curves (`topology.edge`, the object model) should replace
them with the `Curve` methods (`owner`, `evaluate`, `evaluate_point`, `natural_interval`,
`approximate_length`).

Harness: the file-static helpers and local classes of `SmCurve.cpp` (`sm_FlushCrv`,
`sm_AddToCurve`, the stack helpers, `sm_SnapSolToSpecifiedEndpoint`, `sm_UnitizeTangent`,
`sm_GetProjSignedAngle`, `sm_FindTestParams`, `SmLengthIFEO`, `SmCCGlobalSolver`,
`SmCPGlobalSolver`, `SmCurvePropertyGS`) cannot be linked and the file cannot be compiled
inside a namespace (it defines `SmCurve` members), so `harness/cpp/shim/curve_curve_p2.cpp`
holds **verbatim** copies of those line ranges (noted per block) inside a namespace; their
`LocalSolve` members are `SM_ERR` stubs. `sm_TestStep` is global and linked directly. The
solver tests hand-build trees whose `m_pData` is a dummy pointer; the ffi rejects every
branch that would read it (span turning angle, ends, pseudo box).

Upstream behaviour worth knowing:

- **`NER` in `BranchMayContainAnswers`.** With a null `m_pData` the three solvers
  `return SM_ERR_NULL_POINTER` (1006) from a `SmBoolean` function, which every caller reads
  as `TRUE`; the port answers `true`. `SmCPGlobalSolver` checks it *before* the aux-type
  test, `SmCCGlobalSolver` after.
- `SmCurvePropertyGS::BranchMayContainAnswers` evaluates its "corner" points with
  `dZV = 1.0 * j` (not `k`), so only 4 distinct box points are tried, each twice; and the
  `SM_CP_PERPENDICULAR_TO_VECTOR` / `SM_CP_MAXIMA_TO_PLANE` test adds a turning angle in
  degrees to an angle in radians before comparing with `PI/2`. Both kept.
- `SmCurvePropertyGS` declares its own `m_cpOptVectors`, shadowing the base member: the
  property vectors never reach `SmGlobalSolver`'s checks.
- `EuclidianStepOff` returns `SM_ERR` *with* valid outputs (clamped / best parameter) when
  it leaves the interval more than 3 times or does not converge in 30 steps; the port
  returns `Ok` with `StepOff::converged == false`.
- `sm_FlushCrv` returns an exact quadratic arc (when the cubic Bezier polygon lies on a
  circle and the arc matches the span ends within `tol`) *without* clearing the polygon and
  knot arrays; the cubic path clears them.
- `ApproximateLength` and `GetEnds` ignore evaluation statuses (stale or `SM_UNDEF_DOUBLE`
  values); `CreateBySplitCurve` ignores both `Trim` statuses. `SmHermiteCurve` has no
  `Copy`, so splitting one fails with `SM_ERR`; it also inherits the base `GetDegree` (3),
  `GetNumberControlPoints` (0), `GetNumberNaturalKnots` (2) and `IsRational` (`FALSE`).
