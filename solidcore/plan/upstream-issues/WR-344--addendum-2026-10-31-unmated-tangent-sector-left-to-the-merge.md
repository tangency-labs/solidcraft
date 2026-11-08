Updates: WR-344

#### WR-344 (update, 2026-10-31). An unmated edge is left to the merge in the tangent-sector branch too

The 2026-10-30 update leaves an edge whose `MateEdges` failed to the merge only in the extended-convex
branch: `piecewise_merge` then fell back on the upstream `IsTangentSector` test, so a tangent sector
whose mate failed was still listed in `sNoSSI` and its faces' offsets were never intersected. The fork
made the same refinement later, as tangency-labs/smlib@9d8ace9 ("offset: an edge whose faces' offsets
could not be mated is intersected by the merge"), and there the test is
`pE->IsManifold() && !m_sUnmatedEdges.IsIn(pE) && (m_bExtendConvexEdges || IsTangentSector(..))`:
an unmated edge is related in neither branch.

The port now does the same under the same flag, `Kernel::fixes.offset_extended_edges_mated`
(`relates_mated_offsets` in `crates/solidcore/src/offset/offset_executive/p1.rs`). Edges are recorded as
unmated only in `EdgeuseOffset`'s extended-convex branch, so the change can only affect an extended
offset whose `MateEdges` failed at a tangent edge; no fixture is known to reach it. With the flag off
the tangency test alone decides, as upstream. As in the fork, the tangency test is not evaluated for an
unmated edge.

Test: `offset::offset_executive::p1::tests::fork_9d8ace9_unmated_tangent_sector_is_intersected` (fails
with the 2026-10-30 rule, where an unmated tangent sector is related).
