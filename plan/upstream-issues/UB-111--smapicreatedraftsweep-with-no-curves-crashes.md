#### UB-111. `SmApiCreateDraftSweep` with no curves crashes
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SM_API/src/SmApiBrep.cpp:1310` `SmApiCreateDraftSweep` -> `source/SMLib/src/SmPrimitiveCreation.cpp:3604` `SmPrimitiveCreation::CreateTaperExtrude`.
- **What happens:** Unlike `SmApiCreateTaperExtrude` (`:1640`), the draft-sweep wrapper does not reject an empty curve array; `CreateTaperExtrude` has no check for it either and segfaults.
- **Trigger:** `SmApiCreateDraftSweep` with an empty `rCurvesToSweep` (height `1`, angle `5`, caps `3`, either corner type).
- **How found:** adversarial / edge-case fixture (`sweep.api_brep` probe case).
- **Confirmed:** yes.
- **Severity:** crash.
- **Rust port:** `create_draft_sweep` copies the (empty) list as upstream and is a stub until `primitive.primitive_creation_p2` lands. The FFI rejects an empty draft-sweep profile.
