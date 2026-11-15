# Curve length: the speed in difference form moves cases (2026-11-05)

Branch massprops/trim-base-v-default. A B-spline span's speed is now evaluated as
`sum_i R'_i (P_i - P_0)` (`BSplineCurve::span_speed`), with a rounding bound that scales
with the span's size, not its distance from the origin. The integrator accepts a step
within its estimate's rounding only against a final check of the owed errors, and the
final step's last node is the interval's end exactly.

Moves (difftest-speed.log, units curve.curve_p3, curve.curve_p5, mass.integrator,
boolean.api_brep, topology.face_p2):

- curve.curve_p3 gen1005; curve.curve_p5 gen0561, gen2629, gen2638; mass.integrator
  gen0054, gen1177: existing known mismatches whose Rust value moves by at most 1.4e-15
  relative (rounding of the speed). Their recorded errors against the composite
  Gauss-Legendre references are unchanged to the three digits given.
- boolean.api_brep, 9 cases, all `arc_lengths`:
  - box_cylinder_corner_difference, box_cylinder_corner_intersection: two quarter
    circles of radius 3. Rust 4.712388980399728 and 4.712388980399731, against the closed
    form 3 pi / 2: 3.19e-12 relative each, at the requested 1e-9. The rows are sorted
    lexicographically, and the two lengths, equal to rounding, now order the other way
    on the two sides. The point columns (z 5 and 10) swap.
  - box_cylinder_hole_union: four full circles of radius 2. Rust 12.566370614438554 for
    each, 6.3e-12 relative from 4 pi, at 1e-9. The four rows now tie on the length,
    so they sort by z; the C++ lengths differ in the last bits, so its order differs.
  - cylinder_cylinder_tee_{union,difference,intersection}{,_025}: the point at half the
    length of the intersection curve (length 9.88796749071193). Its x (or z) is
    4.9299584956619394e-8 (C++ 4.9299582291755486e-8), or 7.713309526104534e-8
    (C++ 7.713310058988208e-8). That is an absolute difference of 2.7e-15 and
    5.3e-15. The arc-length search stops at `EFF_ZERO_SQRT (len + 1)` on the length,
    about 1e-7, so both sides sit about 5e-8 from the curve's symmetric midpoint alike.
    The value compared is a near-zero coordinate of that search.
- topology.face_p2: no move (47 known mismatches, same values).
