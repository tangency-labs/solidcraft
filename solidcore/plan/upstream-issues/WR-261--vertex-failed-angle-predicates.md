#### WR-261. Vertex tangent predicates consume a failed angle as zero
- **Severity:** Wrong result
- **Area:** Topology and healing
- **How found:** Source audit of #521, with the regression/reference evidence below.
- **Location:** `source/SMLib/src/SmVertex.cpp`, `AreEdgesTangent` and `IsTopologicalVertex`, unchecked `AngleBetween` results.
- **Trigger:** a zero derivative (constant B-spline) or a `1e-12` tangent below the angle routine's scaled zero threshold.
- **What happens:** `AngleBetween` returns 1007 and writes zero; the predicates return a boolean computed from that sentinel. The failed calculation cannot certify tangency or removability.
- **Confirmed:** yes, direct unchanged C++ angle calls reject lengths 0 and 1e-12 (1007, angle 0) and accept length 1 (1000). Native fixtures yield 29 TRUE and 14 FALSE predicates while Rust propagates 1007; 43 explicit known records retain all comparisons. The FALSE results differ only by their lost failure status.
- **Rust port:** propagates tangent and angle errors; related UB-106 and the failed-tangent portion of WR-92 now preserve original evaluation errors too. Cusps retain existing behavior. #521 / PR #700; findings: Failed topology evaluations and incomplete healing stages.
- **Findings:** plan/findings/general--521-topology-evaluation-status.md

**Current PR scope:** The Rust-port paragraph above describes the historical draft.
PR #700 now preserves #704's vertex predicate contract; all 43 former vertex
exceptions were proven matching and removed. See plan/findings/general--521-topology-evaluation-status--addendum-2026-10-24-after-704.md.
