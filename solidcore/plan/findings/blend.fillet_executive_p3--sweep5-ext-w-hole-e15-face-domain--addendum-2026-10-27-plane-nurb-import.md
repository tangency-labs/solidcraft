# blend.fillet_executive_p3: sweep5 ext_w_hole e15/e14 cause corrected: plane NURB dropped by the difftest import (2026-10-27)

Updates: plan/findings/blend.fillet_executive_p3--sweep5-ext-w-hole-e15-face-domain.md

Also covers `sweep5-ext_w_hole-e14-r1.0`
(`plan/findings/blend.fillet_executive_p3--sweep5-ext-w-hole-e14-near-zero-vertex.md`, on
the lazy-extrusion-knots branch).

Unit: `blend.fillet_executive_p3`. Fixture: `fixtures/blend.fillet_executive_p3/sweep_round5.json`.

**Correction.** The original finding misread e15. `result_geometry[442..]` is not one face's
tolerance and UV domain. It is the edge records: two edges sit in a different order in the
result's edge list. In the C++ result the order is rail edge, short edge (interval
`(0, 0.5869)`, tolerance 5.2976e-5), rail edge, short edge. In the Rust result both rail
edges come first. Each side's values are the same edges, shifted.

**Cause.** The harness imports the C++-read body into Rust. The export
(`harness/cpp/shim/sc_edgeuse_graph.h`, `put_surface`, kind `1`) sends only a plane's
analytic data: origin, axes, scales and domain. The import
(`crates/difftest/src/units/topology_vertexuse.rs`, `read_surface`) builds `Plane::new`
without a NURB. The C++ planes do have one: they are made by `CopyAndAddAnalytics` from the
file's B-spline surfaces. When the Rust side later makes the NURB from the analytic
corners, one control point comes out 1 ulp off: y = 2.4999999999999991 against the
C++ 2.5, on the plane of the face with domain u in (5.4380, 7.2407).

The ulp then propagates:
1. `sm_ExtendSurface` of that plane extends a slightly different NURB.
2. `SmConstantRadiusFS::LoadJacobian` gets different `F` and Jacobian values on the
   extended offset surface.
3. `RailEdgeuseIntersect` returns a fillet-vertex UV that is 1 ulp different.
4. The rail natural interval becomes (0, 10) in Rust against (0, 10.000000000000002) in C++.
5. In the stitch, `SelectTopologyToKeep` compares edge-to-face distances of about 1e-15.
   It keeps the target's rail edge where C++ keeps the merged fillet copy, which reorders
   the edge list.

e14's near-zero vertex coordinate is the same kind of rounding.

**Under Rust's own reader.** We built the input with `topology::brep::p7::read_from_file`
(a local prototype, not committed). The planes then keep a NURB that is bit-identical to
the C++ one, and e14 matches C++. e15, and eight other p3 cases, then fail with status
1001 instead. The cause is a second deviation: lazily made NURBs are not cached.
Upstream's `GetOrCreateGwNurbPointer` makes a missing NURB at the first const query
(e.g. `GetNaturalUVDomain`) and keeps it. The port makes it on a copy each time and keeps
none; the plane module docs record this (`crates/solidcore/src/surface/plane/p1.rs`).
Own-read surfaces of extrusion and planes therefore have no NURB, and fillet vertex
points drift by about 1e-15 compared with C++.

The known-mismatch entries stay until one of these changes: the import carries the
NURB, the runner builds its input with Rust's own reader, or the NURB cache is modelled.
