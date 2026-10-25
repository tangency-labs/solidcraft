# heal.stitch_p2 (`SmStitch.cpp` part 2: `DoStitching`, `StitchIntoSolid`, `StitchIntoShell`, one-list `StitchVertexPairs`, `SplitEdgesWithVertices`; PR #205, 2026-10-17; partial, no disagreements)

- **Co-port with `heal.stitch_p1` (#196).** p1 owns the `Stitch` struct, the callback and
  the `SM_API` entry points; p2 adds `impl Stitch` members in `heal::stitch::p2` and wired
  p1's three `UNPORTED(heal.stitch_p2)` API stubs (`SmApiStitchIntoSolid`,
  `SmApiStitchIntoShell`, `SmApiStitchAdvanced`). The harness is p2's own (same B-rep
  blocks and export as p1's, copied, so fixtures are interchangeable).
- **Mostly deferred by design.** `DoStitching` starts with `SmBrep::Notify`
  (`topology.brep_p2`), so every `DoStitching` case, every `StitchIntoShell` case with a
  positive tolerance and the three API entry points defer there; `StitchIntoSolid` defers
  at `SmBrep::CalculateBoundingBox` (`query.brep`). Neither has a passing case. When
  those land, `DoStitching` runs into `GlueVertices` / `FindAndSetInfiniteRegion`
  (`topology.brep_p5`), `SqueezeEdge` / `DeleteFace` (`topology.brep_p3`),
  `GlueEdgesGeneral` (`topology.brep_p7`), `MakeVertexSplitEdge` (`topology.brep_p9`)
  and `SmFace::IsLamina` / `IsDegenerate` / `Notify` (`topology.face_p3`); the runner
  already emits the full outputs and the B-rep digest after the op.
- **What the passes cover.** One-list `StitchVertexPairs`: the sorting, coincidence scan,
  gap bookkeeping, `SelectTopologyToKeep` and the manifold / squeeze skips (the glue is
  deferred). `SplitEdgesWithVertices`: the per-vertex `sm_TestAndSplitEdge` drops, far and
  close-to-boundary outcomes (an interior split defers at `MakeVertexSplitEdge`).
  `StitchIntoShell`: only the path where the doubling loop never runs (`max_tol <= 0`).
- **Guards.** `-3` (upstream never returns): `StitchIntoShell` when `max_tol / 100`
  underflows to zero (HANG-33, confirmed by a C++ run), `StitchIntoSolid` from a zero
  or negative B-rep tolerance (HANG-24; a non-finite diagonal ends or skips the loop).
  The FFI refuses a vertex repeated in the one-list `StitchVertexPairs` list (UB-178).
  Upstream defects kept by the port: WR-186 (`DoStitching`'s sticky candidate flags),
  WR-187 (a cancelled squeeze reported as done), UB-197 (stale edge after a failed glue with `m_bIgnoreProblems`).
