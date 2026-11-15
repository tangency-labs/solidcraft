Updates: WR-663

#### WR-663 (update, 2026-11-15). `query_statuses_pass_on` is removed
The loop, vertex, edge use, face, curve and surface queries now always pass on the statuses SMLib drops, as the fork does (tangency-labs/smlib 7a35f33, 6997962, 666242b, 8843967, 8311f8e, 21dcf69). The classifying queries answer only from what they computed. Propagation is the only path, and the flag is gone (tangency-labs/solidcraft#14246).

Removed:
- the field, SolidCraft's preset line, its entry in `fixes.json` and in `issue_2260`'s list;
- every ignore branch at the sites, and the per-file `pass` wrappers, `point_or_pass_on`, `unitize_or_pass_on`, `ignored_or_soft` and the private helpers that existed only for the flag;
- the control-point bound shortcut of `AssertValid`'s property analyses (`nurbs/b_spline_curve/analysis.rs`, #2568). With the statuses passed on, a search that fails must run so that it reports its failure, which no bound on the curve's speed or radius predicts. The analyses' memo stays.

Tests that compared the flag off and on keep the passed-on half. Tests that pinned a dropped status are re-pinned to the fork's verdict (listed in tangency-labs/solidcraft#16957).

Port corrections made with the removal (each where the flag-on path diverged from the fork or from upstream's own intent):
- **`ComputeCrvsNormal`'s written normal.** `Unitize` writes `(0, 0, 1)` into a zero vector before it returns `SM_ERR_INVALID_INPUT`, and the fork's `SER( crCrvsNormal.Unitize() )` returns with it. The flag-on path returned only the error, so callers that ignore the status (or test `== SM_ERR`, as `CreateCurveSweep` does) read an undefined or zero normal. `compute_crvs_normal_written` hands back the normal as upstream leaves it: zero first, the partial sum after a failed evaluation, `(0, 0, 1)` after a failed `Unitize`. `CreateCurveSweep` keeps it unless the status is `SM_ERR`; `PositionProfileCurves` (unchecked), the taper, the composite chamfer, the analytic rotational sweep and the planar path read it.
- **`SmCPGlobalSolver::Solve`'s closed-curve end continuity.** The fork's `SER` there fails every closed curve that cannot give three derivatives (a conic, a tangent field), so nothing could be dropped onto one. A continuity that cannot be evaluated is no evidence of one: the ends stay discontinuous, upstream's initial value. This deviates from the fork, which is wrong here.
- **`IsLineInParamDomain`'s simple-map copy.** Upstream copies the map with `SmVolume sSimpleVolume(*this, TRUE)`, which slices the derived map off: the plain `SmVolume` cannot evaluate, so the compounded check ran on nothing (with the statuses dropped, `HasPlaneCrossings` read that as in the domain; passed on, as outside it, and the domain-restricted inverses through a bend or unbend then returned points outside the domain as success). The port copies the simple map with its kind, as `IsCurveInParamDomain`'s virtual `Copy(TRUE)` does.

- **Results:** goldens blessed on main and with this change, compared row by row: 740 rows move in 25 units, each listed with its site and verdict in tangency-labs/solidcraft#16957.
