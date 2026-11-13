Updates: WR-75

#### WR-75 (update, 2026-10-29). Tangential curve tracing ported from the fork (opt-in)

The SolidCraft fork's tangential curve tracer (tangency-labs/smlib ef4ab25, e9fde8f, d8c61db and 1693c9d, `SmTangentCurveTracer.cpp`) is ported as the opt-in `Kernel::fixes.ssi_tangent_curve_tracer` (default `false`, upstream behaviour).

- **What it changes:** `DoIntersection` runs a pass after `FindInteriorCurves` (`intersect::surface_intersector::tangent_curves::find_tangent_curves`). Its seeds are:
  - the curve ends where the normals are parallel;
  - the through points;
  - the node pairs `FindInteriorCurves` skips for parallel normals.

  The pass handles each seed as follows:
  - It refines the seed to a contact point (Levenberg-Marquardt on `S1 - S2 = 0`, `n1 -+ n2 = 0`; the solver core is `intersect::surface_intersector::tangent_tracer`).
  - It probes the tangent plane for the directions in which the contact continues.
  - It traces a tangential curve both ways, and the crossing branches through the contact points where crossing curves meet (nodes).
  - It refines spans until `FlushCurve`'s Hermite spans are within tolerance.
  - The new curve replaces the curves and points the marcher produced along it, or is dropped when an existing curve covers it.

  After the degenerate-curve sweep, the nodes are reported as touching points (`add_tangent_nodes`).
- **Corpus:** the fork moves `y23_t2_ex1_tangent_line`, `y23_fig1f_high_order_contact`, `c2n_quartic_circle`, `y23_t2_ex2_tangential_curve`, `y23_fig1d_two_quads` and `y23_t2_ex4_tangential_curve_saddle` from fail to pass (score 81 to 87 of 119, no regression). e9fde8f adds the crossing branches through nodes (`y23_t2_ex3_loop_two_curves`, `c4_steinmetz`'s singular points).
- **Tests:** `crates/solidcore/tests/wr75_ssi_tangent_curves.rs`, both of which fail without the opt-in:
  - `c2n_quartic_circle`: the quartic touching the plane along the unit circle; a tangent curve covers the circle.
  - `c4_steinmetz`: the two crossing points `(0, +-1, 0)` are reported as touching points.
- **Difftest:** no change on the default path. The SSI corpus run uses the default kernel.
