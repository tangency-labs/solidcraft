#### WR-17735. The point/curve Newton solve stops short of a point on a curve whose tangent is shorter than 1, so the point is not found on the curve

- **Severity:** Wrong result (a valid surface of extrusion refused by its own check).
- **Area:** Curves.
- **Location:** `SmFindPCExtremaEFO::Evaluate`, the function `SmCurve::LocalPointSolve` solves
  for `SM_SO_MINIMIZE` (and for `SM_SO_INTERSECT`, which minimises): it scales the gap and the
  derivatives down by tens until `|C'|^2 <= 1e4`, never up. `SmLocalSolve1D::SolveIt` stops
  at `|F| < SM_EFF_ZERO` for `F = (C - P) . C'`, which is the gap along the tangent times
  `|C'|`.
- **What happens:** with `|C'| < 1` that stop accepts a foot up to `SM_EFF_ZERO / |C'|` along
  the curve from the point. On a cubic 0.025 long (`|C'|` about 0.02 to 0.06), about 14 from
  the origin, the solve from the span's chord guess stopped 1.6e-11 to 2.2e-11 from a point
  of the curve. `SmCPGlobalSolver::LocalSolve` drops a solution farther than its tolerance
  from an intersection, so `GlobalPointSolve(SM_SO_INTERSECT)` with
  `SM_EFF_ZERO * (1 + |origin|)` (1.5e-11) found no point. `SmSurfOfExtrusion::AssertValid`
  test 8 ("m_vOrigin is a point on m_pGenCurve") then failed for the extrusion of that cubic,
  though its origin is the cubic's own point at 0.45678 (4e-15 from it in exact arithmetic).
- **Trigger:** SolidCraft #17735: an extruded sketch profile traced from an image, three of
  whose cubic segments are 0.02 to 0.05 long. The extrusion was refused with three test 8
  failures.
- **How found:** the refused extrusion, its three surfaces' generators and origins dumped from
  the check, then the global and local solves traced on each generator: the local solve
  from a nearby guess finds the origin to 1e-15; the span's solve from its chord guess stops
  at `|F| < 1e-12` with the gap 2e-11.
- **Confirmed:** by the code: the port keeps upstream's scaling operations and order for
  finite tangents (`curve.curve_p3` module notes), which only divide.
- **Rust port:** unconditional. `SmCPGlobalSolver::LocalSolve` solves an intersection as a
  minimisation; that minimisation now goes through `Curve::local_point_solve_held` with
  `FindPcExtremaEfo::intersecting` set, and `FindPcExtremaEfo::evaluate`
  (`crates/solidcore/src/curve/curve/p3.rs`) then divides `F` and `F'` by `|C'|` wherever
  the (scaled) tangent is shorter than 1. `F` becomes the gap along the unit tangent, the
  Newton step `-F/F'` is unchanged, and the `|F| < SM_EFF_ZERO` stop leaves the foot at most
  `SM_EFF_ZERO` along the curve from the point (a tangent of length 1 or more already did).
  A closest point keeps upstream's stop. Two rejected designs: scaling the problem up by tens
  until `|C'| >= 1` for every minimisation changed about a hundred golden rows in twenty
  units; doing it for intersections only made `SmSurfaceTracer::IsPointOnCurve` miss a start
  point 0.34 from a traced curve (tolerance 14.3), so that curve was traced twice
  (`intersect.surface_tracer` `gen0211`).
  Goldens: the 20 rows this moves are each an intersection's foot that now lands on the
  closest point, checked outside the solve: the `surface.surf_of_extrusion_p1`
  `review2_par_inv_*` inversions return their case's parameter to 1e-14 (5e-9 before); its
  `gen0361`, `surface.surf_of_revolution_p2` `gen0481`/`gen0624` and
  `intersect.topology_solver_p1` `gen0292` land 1e-18 from the point in exact arithmetic
  (2e-11 to 4e-10 and 5e-12 before); the eight `query.loop` edge parameters leave a gap
  along the edge's tangent below 3e-13 (1.7e-11 to 3.4e-9 before), `query.face_p1`
  `gen0205` 3.6e-12 (1.1e-11 before); and the three `curve.curve_p8` ranges start at the
  exact closest point of one curve's start on the other (`gen2831`: 0.0622 at 0.0866 on
  the first curve, where 0.0708 at its start was reported).
  Test: `a_profile_with_a_short_cubic_extrudes_to_a_valid_solid`
  (`crates/solidcore/src/sweep/extrude.rs`: a cubic 0.025 long closed by its chord, extruded
  1, is one manifold solid that passes `check_brep` level 2; before, test 8 failed).
