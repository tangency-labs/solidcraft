# blend.fillet_executive_p3: rollover-404 mass properties differ by face 11's UV-domain v-min (2026-10-27)

- **Unit:** `blend.fillet_executive_p3` (end-to-end fillet). The difference reaches `mass.integrator` (`SmFace::ComputePreciseProperties`) through the filleted body's face data.
- **Case:** `private-upstream-rollover-404-cyl-on-box1-edge12-r2` (`fixtures/blend.fillet_executive_p3/upstream_rollover_404.json`, upstream `tests/prog_test/src/smFilletTest.cpp:2922`, my_test_suite_4 test 404): `cyl_on_box1.smb`, edge 12, radius 2, then `SmApiBrepComputeMassProperties(body, 1e-4, 1.0, origin 0)`.
- **Values:** `mass_properties[11]` (product of inertia zx): C++ `-3.3651145806047795e-4`, Rust `-3.36511458859837e-4` (rel 2.4e-9, above the 1e-9 gate). Topology, the geometry record, area, volume, mass, centroid and the other products and moments agree.
- **Root cause:** face 11 (0-based, `GetFaces` / `p1::faces` order) of the filleted body stores a UV domain whose v-min is `-2.0000000000000013` in Rust and exactly `-2` in C++ (about 6 ulp).
  - `SmFace::ComputePreciseProperties` integrates each iso line from the face's v-min (`GetUVDomain().GetMin().y`, Rust `uv_domain.min().y`). That lower bound moves every inner integral by a few ulps: face 11's area by 2 ulp, its moments more.
- **What is identical** (pin v1.0.1 diagnostics in pin101-tests, gdb on both sides):
  - the B-rep box and every face box, bit for bit;
  - every other face's precise properties, bit for bit;
  - face 11's trim curves: the points and tangents of the first integrand calls agree to every digit, e.g. `(10.267949192431121, 4.9999999999999991)`;
  - its knot spans, error scales, first divisors and step counts.

  So the trims and the integrator port are faithful.
- **Why it shows at 1.0.1:** SMLib v1.0.1's `SmApiBrepComputeMassProperties` integrates about the B-rep box midpoint and shifts the moments to the caller's origin (`SmApiQueries.cpp:991`-`1002` at `3fd5b95`). The zx product is a cancellation of terms about 1e3 larger, so the few-ulp drift reaches 2.4e-9 relative. At `c9c5979` it stayed under the gate.
- **Status:** a Rust divergence, not a C++ defect (no `cpp_defect`); a known mismatch until fixed.
- **Follow-up:**
  - Find where face 11's `uv_domain` is set in the fillet pipeline (candidates: `SmFace::SetUVDomain` from the trims' UV bounding box, the fillet face's `MakeFaceBrep`, the trim and stitch steps). A gdb watch on that face's domain is the first step.
  - Add the face UV domain to the `blend.fillet_executive_p3` end-to-end record, so a difference there is caught directly (a harness change, not part of the pin PR).
