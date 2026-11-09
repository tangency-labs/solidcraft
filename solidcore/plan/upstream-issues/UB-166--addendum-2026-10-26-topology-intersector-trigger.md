Updates: UB-166

#### UB-166 (update, 2026-10-26). A trigger through `SmTopologyIntersector::IntersectTopology`
- **More triggers:** `--fuzz 30000` of `boolean.topology_intersector_p2` (2026-10-21), `fuzz23120`: a B-rep face whose B-spline surface has the v knot vector's first interior knot set to `1e-300` (`fixup_split`, `iir`, `edge_edge`); segfault in `sm_DecomposeSrf` <- `SmSurfaceCache::BuildTreeBase` <- `SmFace::FaceIntersect` <- `SmTopologyIntersector::IntersectTopology` (gdb).
- **Rust port:** the `boolean.topology_intersector_p1` shim (shared by `_p2`) reads back the knot vectors the surface constructor left and refuses a B-rep with such a surface (`SC_ERR_ARG`), as the `blend.fillet_geom_p1` shim does. Regression input: `crates/difftest/tests/fuzz_regressions/boolean.topology_intersector_p2.json`.
