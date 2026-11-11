#### UB-175 (addendum). Tiny Booleans reach the teardown through the public API
- **Severity:** Crash / undefined behaviour
- **Area:** Surface
- **Trigger:** two boxes smaller than the proto vertex capture distance
  (`max(0.01, 2000 * tol)`, `SmTopologyIntersector.cpp:1159`), e.g. `boolean.api_brep`
  `gen0059` (`with_options`, operation 3, boxes about 0.005) and `gen0066` (`merge_breps`,
  operation 2, boxes about 0.008 and 0.004): the intersection points collapse into shared
  proto vertices, so proto edges start and end on one of them, and the manager's
  destructor reads the freed edges.
- **How found:** once `SmEdgeDefinition::Imprint` ran `SmBrep::MakeEdgeInFace` in the
  port, these cases reached the port's UB-175 check; the C++ goes on (results of 8 and 7
  records).
- **Confirmed:** by the port's teardown replay only; the C++ build was not instrumented.
