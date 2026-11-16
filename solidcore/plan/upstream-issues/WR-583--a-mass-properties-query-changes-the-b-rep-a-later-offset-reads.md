#### WR-583. A mass-properties query changes the B-rep, so a later shell of it differs
- **Severity:** Wrong result
- **Area:** Offsets and shells
- **Location:** `source/SMLib/src/SmFace.cpp`, `SmFace::ComputePreciseProperties` (def. `:6144`) calls `CreateOrientedTrimBoundary` (`:6176`; def. `:7333`), which runs `CreateUVTrimCurves(FALSE, ..)` (`:7361`; def. `:8106`) on a face without UV trims, and that runs `sm_FixUVTrimCurves` (`:8483`). Also `source/SMLib/src/SmVertexuse.cpp`, `SmVertexuse::GetVertexFaceGap` (def. `:1668`), which caches the gap on the vertex use. Reached from `SmApiBrepComputeVolume` (`source/SM_API/src/SmApiQueries.cpp:835`).
- **What happens:** a volume or mass-properties query, which reads as a pure query, changes the B-rep:
  - it creates the missing UV trim curves;
  - `sm_FixUVTrimCurves` moves an existing trim's end control point onto its neighbour (4.8e-12 → 0.0 here);
  - vertex uses keep a vertex/face gap record.

  A later offset or shell of the same B-rep reads these trims and gaps. Its result therefore depends on whether the body was measured first: the shell's volume differs in its last digits, and a shell can go from built to refused.
- **Trigger:** tangency-labs/solidcore#1528's plate:
  - a 27.5 thick sweep of three straight sides and a half circle r 10, minus an r 5 blind hole along x through the round side;
  - `SmApiShellBrepFull` 2 inward, open at the far end;
  - with and without a prior `SmApiBrepComputeVolume` on the operand.

  With the fork's loop-gap closing (solidcore's WR-578 opt-in), the shell builds on a clean operand and is refused after the query.
- **How found:** a probe that measured the operand before shelling disagreed with a test that did not. The difference was traced to the writes above (debugger trace of the volume query's writers).
- **Confirmed:** yes. fixer-8's operand-state extension (#1793) of case `issue1528_plate_api_shell_2` of `offset.offset_executive_p2` (from #1757) shows the pinned C++ making the same writes and shelling to the same, shifted volume as the port, with and without the query.
