Updates: WR-453

#### WR-453 (update, 2026-10-27). Rechecked against v1.0.1 (3fd5b95)

The defect is still in v1.0.1: `SmEdgeDefinition::Imprint` (`SmProtoTopology.cpp:1375`, was `:1369`) makes the edge over the interval and curves it was given (`MakeEdgeInFace` at `:1462` and `:1541`). The new drop-distance test in `SmEdgeDefinition::IsInteriorPoint` (`:1343`-`:1348`) only rejects trimming to a point farther than the tolerance from the curve; it does not continue a curve to a vertex past its end.

Rust: on `main` with the v1.0.1 port, `extended_curve_continues_the_end_span_polynomial` and the two `issue_435_*` tests in `crates/solidcore/tests/issue_410_435_thread_groove.rs` (which run with `imprint_extends_edge_to_proto_vertex` on and off) still pass. The default path agrees with the C++ 1.0.1 in difftest (`topology.proto_topology_p1`, `_p2`, `_p3`: all cases pass, max_rel_err 0). **Confirmed:** still no, as in the record: the C++ 1.0.1 was not run on a nut-and-groove input.
