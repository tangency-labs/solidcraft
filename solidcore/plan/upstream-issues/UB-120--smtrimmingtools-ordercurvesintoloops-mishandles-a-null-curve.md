#### UB-120. `SmTrimmingTools::OrderCurvesIntoLoops` mishandles a NULL curve
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmTrimmingTools.cpp:1800` (`if (pC == NULL) continue;`), `:1890` (`sVertexPoints[2*nCurveIndex]`), `:1871` (`if(pC == NULL) break;`), `:2078` (`pC->IsOnPlane(...)`).
- **What happens:** A NULL entry adds no end points, so every later curve reads the end points of the curve after it (and the last ones read past the array); in the loop search a NULL curve that is the first unseen one ends the search early; the planarity test finally dereferences it.
- **Trigger:** `crCurves` with a NULL entry.
- **How found:** read during porting (unit `topology.trimming_tools`).
- **Confirmed:** no (the difftest wrapper never passes NULL curves).
- **Severity:** crash.
- **Rust port:** takes a slice of curves (no NULL entries).
