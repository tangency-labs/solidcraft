#### WR-771. The projected curve/curve maximum returns stationary pairs that are not maxima, or no answer

- **Severity:** Wrong result
- **Area:** Curves
- **Location:** `source/SMLib/src/SmCurve.cpp:10536-10834`, `SmCurve::GlobalCurveSolve`, for `SM_SO_PROJECTED_MAXIMIZE`, at v1.0.1 (`3fd5b9548d19aa8c12c29bc9637a96d42a06f8a5`). The global solver subdivides both curves' trees and keeps whatever its local solves converge to.
- **What happens:** A local solve that reaches a pair where the projected stationary equations vanish is accepted as the answer. That includes coincident pairs and other stationary pairs that are not maxima. Nothing proves that no other pair of the two curves projects farther apart. Some queries return success with no answer at all.
- **Trigger:** Six of the `curve.curve_p9` cases from issue [#13698](https://github.com/tangency-labs/solidcraft/issues/13698), with the largest value reported on main and an admissible pair that is larger:

  | Case | Reported | Admissible pair | Its projected distance |
  |---|---:|---|---:|
  | gen0844 | 0 | (-2.44896896704292, 3.043140518489156) | 10.979584714079621 |
  | gen1839 | 0 | (0.43561427937496744, 0.03474215986040488) | 4.40801559960347 |
  | gen2050 | 6.1371363e-8 | (0.42689846142600174, 0.30573599521598394) | 40.00123197279587 |
  | gen2187 | 0.09955759757712898 | (3.254294670614625, -3.304387257364806) | 6.310837322899527 |
  | gen2585 | 0.000128676797868 | (0.6795950883084139, 1.0610357492278748) | 0.9962268759220879 |
  | gen0472 | 5.0105715611725925 | ellipse angles 0 and 90 degrees | 5.346613700814649 |

  In all, 35 `curve.curve_p9` maxima on main are either empty (8) or below an admissible pair (25), and the four cylinder and half-cylinder curve/face pairs `analytic_face_{cyl_x,halfcyl_x}_{6,7}_15` report 0.5 where sqrt(2.5) is attained.
- **How found:** The caller audit for PR [#2840](https://github.com/tangency-labs/solidcore/pull/2840), recorded as issue #13698.
- **Confirmed:** in solidcore's port of this path, which keeps the C++ algorithm: the reported values are main's goldens, and the admissible pairs are the witnesses recorded in #13698 (re-measured on the native curves in this PR's finding). No C++ build was run, so the C++ outputs themselves are inferred from the port, not observed.

The Rust fix (tangency-labs/solidcore#3077) replaces the projected-maximum Single request, and two structurally selected All classes, with a construction that encloses the objective over every parameter rectangle and returns a pair only when the global upper bound fits that pair's own arithmetic enclosure; otherwise it refuses.
