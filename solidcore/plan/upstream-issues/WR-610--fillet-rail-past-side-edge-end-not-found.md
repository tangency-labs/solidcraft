#### WR-610. `RailEdgeuseIntersect` bounds a straight side edge to its own interval, so a rail meeting its line past the end is not found
- **Severity:** Wrong result
- **Area:** Blend (fillet solver)
- **Location:** `source/SMLib/src/SmFilletSolver.cpp`, `SmFilletSolver::RailEdgeuseIntersect` (def. `:3262`): `sIntervals[0] = pUVCurve->GetNaturalInterval()` (`:3344`) bounds the side edge use's parameter in the solve.
- **What happens:** a rail that leaves its face across a cliff meets the side edge's line past the edge's end. Bounded to the edge's interval, the solve finds that point only from a first guess already on it (`SmFilletVertex::CalcRailIntEdgeuse` guesses `r` along the edge only while the edge is longer than `r / 2`), so the fillet has no rail ends and `CalcFilletGeom` fails.
- **Trigger:** a round r10 on a 5 x 5 bar's long edge, wider than twice the faces: fillet error 4, "Fillet Surface Computation Failure" (r9, whose guess is made, gets as far as the corner edges, error 3).
- **How found:** SolidCraft (#960, #1466). The fork's fix is tangency-labs/smlib@6e77631 (rev `68fb9162`), which is not merged upstream; the same commit moves `sm_ExtendTraceToItsRails` (WR-609) before the corners' vertices.
- **Confirmed:** no C++ run of the fork was made. The Rust port follows the pinned C++ (`3fd5b95`) with the flag off: r10 fails with error 4.
- **Rust port:** behaves as upstream by default. With `Kernel::fixes.fillet_trace_reaches_rails` (shared with WR-609), a side edge whose curve and UV trim curve are linear on a planar base surface has its parameter interval widened past both ends by the larger of the trace's reach and the extension factor times the edge's length (`blend/fillet_solver/p2.rs`, `run_side_edge_on_along_its_line`). The trace reach is set for every solver before the corners (`blend/fillet_executive/p3.rs`, `create_fillet_brep`).

  Tests: `bar_round_wider_than_its_faces_finds_its_rail_ends` in `crates/solidcore/tests/wr609_trace_reaches_rails.rs` (r10 fails with error 4 without the flag, and with it gets as far as r9, error 3; the cliff overflow of #960's later PRs builds it).
