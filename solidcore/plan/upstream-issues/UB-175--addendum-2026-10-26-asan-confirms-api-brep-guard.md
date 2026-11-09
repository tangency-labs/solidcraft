Updates: UB-175

#### UB-175 (addendum). AddressSanitizer confirms the `boolean.api_brep` guard
- **Severity:** Crash / undefined behaviour
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmProtoTopology.cpp`: the read in
  `SmProtoEdge::RemoveProtoVertex` (:3371) from `SmProtoVertex::~SmProtoVertex` (:1893),
  run by the `SmObjsDelete<SmProtoVertex*>` of `~SmProtoTopologyManager`. The memory was
  freed by `SmProtoEdge::~SmProtoEdge` (:2673).
- **Trigger:** as in the original record. It is also reached after a failed
  `ImprintIntersections` that edited a B-rep (UB-242). Upstream only warns there and goes on
  through `SetRelationships` to the same `delete m_pProtoTopoMgr`
  (`SmTopologyIntersector.cpp:1267`). That delete also runs when `SetRelationships` fails.
  It does not run after a failed `IntersectTopology` or `ResolveIntersections`: `SER_MSG`
  returns first and the manager is leaked, so those paths have no teardown. Fixture
  `ub175_after_editing_imprint_failure` (`fixtures/boolean.api_brep/ub175_teardown_paths.json`)
  reaches the UB-242 path. No generated input among 4000 seeds reaches the
  `SetRelationships`-failure path.
- **How found:** SMLib, SM_API and the `boolean.api_brep` shim were rebuilt at the pinned
  commit with `-O1 -g -fsanitize=address` (other flags and defines as in `ffi.env`). A small
  driver ran every case of a default `boolean.api_brep` run plus that fixture, using the
  exact FFI encoding and `ASAN_OPTIONS=detect_leaks=0:halt_on_error=1`.
- **Confirmed:** yes, by the C++ build under AddressSanitizer. Every input the port's teardown
  replay flags hit the heap-use-after-free above: `gen0047`, `gen0059`, `gen0066`, `gen0071`,
  `gen0150`, `gen0201`, `gen0235`, `gen0239` and `ub175_after_editing_imprint_failure`. All 287
  accepted cases ran with no sanitizer report. `gen0071` and `gen0150` (negative-radius
  spheres) and the non-tiny `gen0150`/`gen0201` hit the same read, so the guard is input-exact
  on this set.
