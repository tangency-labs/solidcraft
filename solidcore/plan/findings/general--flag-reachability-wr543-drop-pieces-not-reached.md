# General: WR-543 `uv_trim_joins_drop_pieces` is reached by no operation test (2026-10-31)

- **Flag:** `Kernel::fixes.uv_trim_joins_drop_pieces` (WR-543, #1383; fork tangency-labs/smlib@e36feb1). The guard is in `topology/edgeuse/p3.rs` `create_uv_trim_curve`, the more-than-one-piece block.
- **Audit:** `plan/findings/audits/flag-reachability-2026-10-31/` classifies it as C. `longest_drop_run_joins_the_pieces_that_meet` tests the helper with the default kernel only, so the call-site guard is never run with the flag on.
- **Not done here:** no operation-level geometry was built. The record's trigger is a creased rational bicubic patch with an edge along its boundary row on the edge's own non-uniform parameter, starting 0.05 mm past the patch (trim 0.85 mm off the edge; up to 5 mm on SolidCraft's Poppy Cut-Extrude10). It needs an edge made on such a face (a make-edge-in-face or imprint call), which the audit's follow-up did not get to.
- **Next step:** build that patch and edge, call the operation that makes the edge's UV trim with the flag on, and assert the trim's distance from the edge (under the zone tolerance with the fix, 0.85 mm without).
