#### WR-54. Old composite curve records get their segment gaps shifted by one
- **Severity:** Wrong result
- **Area:** Topology, attributes, solvers
- **Location:** `source/SMLib/src/SmBrepData.cpp:5438` (`sGaps` holds the first segment's start gap, then every end gap) passed to `SmCompositeCurve.cpp:1050` (which reads `(*cpGaps)[i]` as segment `i`'s end gap).
- **What happens:** For a version 32 or older composite curve, segment `i` gets the stored end gap of segment `i - 1` as its end gap (segment 0 gets its own start gap); the last end gap is never used. The branch is marked "untested code" upstream.
- **Trigger:** A closed two-segment composite record with gaps `0.5 0.25` and `0 3`: segment 0 gets gaps `0.25 / 0.5` instead of `0.5 / 0.25`.
- **How found:** read during porting (`topology.brep_data`).
- **Confirmed:** no (gaps are not in the difftest dump).
- **Severity:** wrong result.
- **Rust port:** Faithful to the code.
