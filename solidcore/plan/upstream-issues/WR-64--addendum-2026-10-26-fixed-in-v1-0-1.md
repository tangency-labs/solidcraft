Updates: WR-64

#### WR-64 (update, 2026-10-26). Fixed in v1.0.1

Fixed in v1.0.1 (3fd5b95): sm_TotalCoincidenceChecker (SmCone.cpp), the reversed line test now measures sPt01 to sPt10 and a non-coincident line returns SM_SUCCESS instead of FALSE. Verification: surface.cone_p1 coincidence cases, against the v1.0.1 shim copy (harness/cpp/shim/surface_cone_p1.cpp), pending the pin101 difftest run.
