#### WR-550. `StepBackFromSingularity` overwrites a pole point's exact tangent with its neighbour's
- **Severity:** Inexact result (a traced curve 2.02e-4 off its surface at a tolerance of 4e-5)
- **Area:** Intersections (surface intersector)
- **Location:** `source/SMLib/src/SmSurfaceIntersector.cpp`, `SmSurfaceIntersector::StepBackFromSingularity` (`:2332` in the fork): it first copies one point's curve tangent over the other's.
- **What happens:** `ComputePointValues` gives a point at a surface pole its curve tangent from the pole's limit normal (`EvaluateNormal`), which is exact there. `StepBackFromSingularity` is called next only to get usable surface derivatives just off the pole, but it overwrote that tangent with the previous trace point's. The Hermite span into the pole was built with the wrong end tangent, and `FlushCurve`'s merge of a short last step stretched that span to a full step. A line projected through a sphere's pole (r 20, tolerance 4e-5) came out 2.02e-4 off the sphere: the pole tangent was (1, 0, -7.5e-4) instead of (1, 0, 0) over a 1.75 mm span.
- **Trigger:** a parallel projection (or any trace) running through a sphere's pole.
- **How found:** SolidCraft. The fork's fix is tangency-labs/smlib@8d95600 ("a curve traced into a surface pole keeps the pole's tangent"), which is not merged upstream.
- **Confirmed:** no C++ run was made here. The Rust port follows the pinned C++ (`3fd5b95`).
- **Rust port:** behaves as upstream by default. With `Kernel::fixes.ssi_pole_point_keeps_its_tangent`, `step_back_from_singularity_points` leaves both points' curve tangents as they are; only the surface derivatives (and singular parameter) come from the stepped-back point.

  Test: `tests/wr_550_projection_through_sphere_poles.rs`.
