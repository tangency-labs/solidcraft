# offset.offset_executive_p1: RemoveExcessFaces correspondence forgetting is upstream in v1.0.1 (2026-10-26)

Updates: plan/findings/offset.offset_executive_p1--remove-excess-faces-correspondence-maps-issue-368.md

With the reference moved to v1.0.1 (3fd5b95), the #368 change is upstream behaviour and
no longer a fork-only port deviation. Two claims of the original finding are superseded:

- **"forgets every candidate face before calling `RemoveFaces`".** v1.0.1
  `SmOffsetExecutive::RemoveExcessFaces` forgets after `RemoveFaces`, and only when some
  face is deleted. The relations are keyed by face, so the result is the same set of
  pairs. Rust: `OffsetExecutive::remove_excess_faces` now removes first, then calls
  `forget_faces`. `forget_faces` takes a `BTreeSet` and calls `forget_seconds` on the
  four face relations, then `forget_firsts` on `blend_to_faces`.
- **"The pinned reference leaves stale face pointers for `PiecewiseMerge`".** True at
  c9c5979 only. v1.0.1 also prunes `m_vEUToEU` of the removed faces' edge uses and of
  any edge use no result edge has (`sm_ForgetSeconds(m_vEUToEU, sGone, &sLive)`). The
  port does this too (`removed_faces_edgeuses`, `forget_seconds`, with the test
  `forget_seconds_drops_removed_and_dead_edgeuses`).

The module's Deviations note about #368 is removed. The validation counts in the
original finding are for c9c5979; the v1.0.1 counts come from the pin rerun. Upstream
records: UB-258 and UB-256 (the `PiecewiseMerge` UAF) addenda, both fixed in v1.0.1.
