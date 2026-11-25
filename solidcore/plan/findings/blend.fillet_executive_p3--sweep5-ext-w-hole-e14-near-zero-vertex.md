# blend.fillet_executive_p3: sweep5 ext_w_hole e14 near-zero vertex coordinate (2026-10-27)

Unit: `blend.fillet_executive_p3`. Fixture: `fixtures/blend.fillet_executive_p3/sweep_round5.json`.

Input: `sweep5-ext_w_hole-e14-r1.0` (`Fillet/ext_w_hole.smb`, edge 14, circular G1, radius
1.0, thumbweight 1). It completes once the lazy surface-of-extrusion knots are made
(`plan/findings/blend.fillet_executive_p3--sweep5-lazy-extrusion-knots--addendum-2026-10-27-resolved.md`).
Before that, Rust refused it at the stitch to the original B-rep.

Values: everything agrees (status, topology, mass) except `result_geometry[62]`, the z
coordinate of result vertex 15: C++ 1.4453956777755315e-15, Rust 3.6012056774890033e-16.
The body's coordinates are of order 10, so both are rounding noise around 0. The relative
comparison has no absolute floor for this key, so they do not compare equal.

Suspected cause: a different operation order (rounding) in the computation of this vertex
point, e.g. an intersection or a fillet-vertex solve. Not localised. Per the correctness
gate, no absolute floor is added.
