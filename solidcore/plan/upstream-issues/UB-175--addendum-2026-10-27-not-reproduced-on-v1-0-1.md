Updates: UB-175

#### UB-175 (addendum). Not reproduced on v1.0.1 under AddressSanitizer
- **Severity:** Crash / undefined behaviour
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmProtoTopology.cpp`, `SmProtoTopologyManager::~SmProtoTopologyManager`
  (v1.0.1 clears every proto edge's end vertices and every proto vertex's edge list before the
  teardown deletes them).
- **Change:** with the reference pinned to v1.0.1 (3fd5b95), the `boolean.api_brep` runner no longer
  keeps the inputs that reached this use-after-free on v1.0.0 from the C++ side; they now run the
  real SmApi Boolean and its proto topology teardown, and are compared like any other case. Its
  guard now rejects only the inputs that reach UB-242 (a failed imprint that may have edited a
  B-rep, where the port stops and upstream goes on); on v1.0.0 those also reached this teardown. The
  `boolean.topology_intersector_p1`/`_p2` shim no longer unlinks stale proto edge entries before
  deleting a proto topology manager, and its runner no longer answers `-2` for an
  `IntersectInsertRelate` the port records UB-175 for (the UB-242 stop is unchanged).
- **How checked:** SMLib, SM_API and the difftest shims were rebuilt at 3fd5b95 with AddressSanitizer
  (the flags and defines of `ffi.env` otherwise unchanged). A default `boolean.api_brep` run and a
  4000-seed run, which include every input the earlier addenda found reaching the teardown, ran
  with no sanitizer report.
- **Status:** not reproduced on v1.0.1; the difftest guard for it is removed. The record stays: it describes
  a defect of v1.0.0.
