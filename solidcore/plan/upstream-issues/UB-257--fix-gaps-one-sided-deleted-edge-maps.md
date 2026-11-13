#### UB-257. FixGaps retains a one-sided deleted edge in its mate maps
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmTopologyIntersector.cpp:713`, `FixGaps`.
- **Trigger:** `CombineCoincidentVertices` deletes an edge in only one operand.
- **What happens:** the `pDeletedBrepEdge && pDeletedOtherEdge` guard skips cleanup; a
  later `GetCommonEdges`/`Mark` reads freed memory (reported under ASan in #432).
- **Confirmed:** no local C++ rerun. The Rust regression reproduces the stale map
  before the fix; the C++ memory error was reported under ASan in #432.
- **Rust port:** Fixed in tangency-labs/smlib be3fc10; the port carries it, with
  the safety deviation documented in the module header and `plan/findings/boolean.topology_intersector_p2--independently-deleted-gap-edges.md`.
- **How found:** ASan report in [#432](https://github.com/tangency-labs/solidcore/issues/432);
  the Rust stale-map regression is in [#642](https://github.com/tangency-labs/solidcore/pull/642).
