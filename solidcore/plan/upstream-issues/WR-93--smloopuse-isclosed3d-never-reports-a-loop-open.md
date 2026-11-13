#### WR-93. `SmLoopuse::IsClosed3d` never reports a loop open
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `SmLoopuse.cpp:873`-`885` (`SmLoopuse::IsClosed3d`; also reached through `SmLoop::IsClosed3d`, `SmLoop.cpp:248`).
- **What happens:** For a gap over its cross-section tolerance it computes `dOverTol = sXSectTol3d - sqrt(dGap3dSq)`, which is negative there, and keeps it only when `dOverTol > dMaxOverTol` (initially `0.0`). No gap ever qualifies, so `pMaxOverTolEdgeuse` stays `NULL` and the loop is always reported closed, with the largest gap and its tolerance as outputs. `bCheckAllGaps == FALSE` still stops the scan at the first gap over tolerance, so the reported "largest" gap is then only the largest seen so far.
- **Trigger:** Any edge loop with an end-to-end gap larger than the sum of the two edge uses' tolerances.
- **How found:** read during porting; the difftest compares it (unit `topology.loopuse`, `closed3d` cases).
- **Confirmed:** yes: the `closed3d` cases of `fixtures/topology.loopuse/tolerance_limit.json` set every edge tolerance to 0, 1e-15 or -1e-12 so that real end gaps exceed `XSectTol3d`; both sides still report those loops closed (with the largest gap seen, also when `bCheckAllGaps == FALSE` stops early).
- **Severity:** wrong result.
- **Rust port:** Reproduces it (`topology::loopuse::is_closed_3d`).
