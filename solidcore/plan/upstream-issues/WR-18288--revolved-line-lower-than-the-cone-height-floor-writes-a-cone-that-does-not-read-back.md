#### WR-18288. A revolved line that rises less than the cone recogniser's height floor is written as a cone that does not read back
- **Severity:** Wrong result (a body Measure and Fillet cannot read)
- **Area:** Sweeps (rotational sweep, analytic faces)
- **Location:** the SolidCraft fork's `SmRotationalSweepGeometry::AnalyticSurface`, the line
  branch (tangency-labs/smlib 0b143f7, 9853e54), ported as `sweep/analytic_revolution.rs`
  `candidate` (WR-448). It builds a cone for any line not perpendicular to the axis within the
  sweep's tolerance. `SmCone::IsNurbSurfaceCone` (solidcore `surface/cone/p2.rs`
  `Cone::is_nurb_surface_cone`) refuses a NURB whose end circles' centres are less than
  `SM_EFF_ZERO_SQRT` (1e-6) apart, calling it a disk. Pinned SMLib gives such a curve of a
  longer profile an `SmSurfOfRevolution` on a whole turn and a B-spline surface of revolution
  on a partial one (WR-448).
- **What happens:** a line almost square to the axis, rising along it by more than the sweep's
  fit tolerance (about `1e-9 (1 + size)`) and less than 1e-6, gets a cone face, unless the
  caller first takes it as perpendicular and makes a plane: a whole turn tests it against
  `angle_tol = tolerance / length_scale` (`primitive/primitive_creation/p3/sweeps.rs`), a
  partial turn against `EFF_ZERO_DEG` (`sweep/rotational_sweep_geometry.rs`). A reader that
  rebuilds the analytic surface from the face's NURB with `IsNurbSurfaceCone` (SolidCraft's
  flat-array record) refuses it: "a NURBS form that is not its analytic surface". The sweep
  reports success.
- **Trigger:** a full turn about X of the closed profile (0, 0), (6, 0), (6, 1),
  (6.0000001, 1.01), (5, 2.5), (0, 1) on XZ: the edge from (6, 1) rises 1e-7 along the axis.
  tangency-labs/solidcraft#17085.
- **How found:** tangency-labs/solidcraft#17085.
- **Confirmed:** no C++ or fork run here (the fork's line branch is read from the port, WR-448);
  the port shows it: Measure of the trigger's body fails without the fix.
- **Fix (always on, no flag):** `candidate` gives no analytic surface for a line, not parallel
  to the axis, whose height along the axis is below `Cone::MIN_HEIGHT`, the recogniser's own
  floor (one named constant, so the two cannot drift). The caller keeps its own surface,
  which reads back: a whole turn its surface of revolution, a partial turn its B-spline
  surface of revolution, run through `copy_and_add_analytics` when analytics are asked for. Only such lines change. A cylinder is not affected.
- **Not fixed here:** a cone at or above the floor that `IsNurbSurfaceCone` still refuses. By
  its other tests: its axis, taken from the end circles' centres, must be parallel to the
  circles' normal within `SM_EFF_ZERO_SQRT` degrees, which a short cone far from the origin or
  of a large radius misses by rounding. And at the floor itself: the sweep measures the height
  along its axis, the recogniser between the NURB's circle centres, so a cone just at the floor
  for the sweep can be just under it for the recogniser.
- **Tests:** `sweep::analytic_revolution::tests::a_cone_lower_than_the_recogniser_floor_keeps_the_surface_of_revolution`;
  SolidCraft's `crates/engine/src/tests.rs`
  `revolved_edge_almost_square_to_the_axis_reads_back` (full and quarter turns, volume against
  Pappus).
