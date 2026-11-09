# topology.topology (green; no disagreements)

657 difftest cases (57 fixtures + 600 generated) agree exactly. Upstream behaviour
worth knowing:

- **Callback support added by PR #194.** The original call graph classified
  `SmTopologyChangeCallback::{Execute, AddUnique, Remove}` as unused, so the
  original unit compared only empty tracker arrays on bare `SmTopology` objects.
  The B-rep part 5 healer integration now co-ports those methods and compares real
  face/edge/vertex callback scripts separately. Geometry walks require actual
  owner metadata, with a 64-link bound; missing metadata is a capture failure.
  Tracker/callback state uses a private shared notification-array allocation so
  active resets and foreign-callback replacement preserve native array lifetimes.
  `topology.brep_p3` (PR #204) runs heal_data_p3's ported edits (`SqueezeEdge`,
  `DeleteFace`) under this tracker too (`SmHealData` installs
  `SmTrackTopologyChanges(m_pBrep, TRUE)` around them, SmHealData.cpp:3341).
- **Starting a tracker twice leaves its callback installed after it dies.** The
  second `StartTracking` sees the tracker's own callback in the slot and stores it in
  `m_pLastNotifyCallback`; `StopTracking` (and so the destructor) then does nothing,
  and the context keeps a dangling `SmNotifyCallback*`. The next `Notify` calls
  `Execute` on a destroyed object; a later tracker allocated at the same address is
  even reported as tracking. The shim skips (`ST_SKIP`) destroying a tracker in that
  state, and the Rust runner applies the same rule. Rust's tracker recognises its
  callback by a caller-chosen key, so address reuse cannot fake `IsTracking`.
- **`StopTracking` clears whatever callback is installed** when this tracker found
  the slot empty at start, including a foreign one installed since (fixture
  `track_stop_clears_foreign`). Ported as is.
- **Notify asymmetries** (ported as is): a merge ORs the two targets' flags into both
  but copies no flags from the source; a copy (`SM_NO_COPY`, or `SM_NO_CONSTRUCTION`
  with a source) zeroes the target's three general marks while a split copies them;
  `m_lMarkIO` and `m_lMarkAssert` are never propagated; `SM_NO_MERGE_IN_BREP` takes the
  *deleted* object (`pData2`) as the source (the upstream "GWC:TODO try typo bug fix").
- `SmTol::GetZoneTol3d(const SmObject*)` and friends dispatch to the *derived*
  `GetTolerance` overrides; their `UNPORTED` markers now name `topology.vertex`,
  `topology.edge`, `topology.face_p2`, `topology.edgeuse_p1` and `topology.brep_p1`
  instead of this unit.
