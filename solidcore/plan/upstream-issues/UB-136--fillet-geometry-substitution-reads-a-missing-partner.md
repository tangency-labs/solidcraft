#### UB-136. Fillet geometry substitution reads a missing partner interval
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmCurveClass.cpp:16830` (`sm_SetDoFilletingGeometry`), called by `SmCurveClassification::MergeInterval`; unchecked indexing in `source/SMLib/inc/SmCurveClass.h:508`.
- **What happens:** Fillet merging indexes both classifications at the target interval without checking the paired classification's size. An empty partner yields a null read; a shorter partner permits an out-of-range interval read.
- **Trigger:** `a = [0,.5] UNKNOWN/UNKNOWN/UNKNOWN + [.5,1] VERTEX/EDGE/VERTEX`, target index 1, empty `b`, flags 2 or 3. FACE and REGION interiors also reach the same invalid access. A one-interval partner at index 1 is invalid even when it does not crash.
- **How found:** maintainer review of PR #138; the existing matrix paired identical one-interval arrays and used index 0.
- **Confirmed:** yes, the maintainer reproduced SIGSEGV for all five curve kinds in full and vertices-only modes (13 crashing probes in total), plus the one-interval out-of-range read.
- **Severity:** crash / out-of-range read.
- **Rust port:** Kernel topology mutation remains deferred. Both the safe FFI validator and the C++ shim require equally sized partners for op 3 with fillets. Six `rejected_fillets_*` fixtures check rejection; 660 isolated probes independently exercise both guards at first and last indices of 1/2/3/32 intervals. Valid equal-sized pairs and non-fillet calls remain accepted.
