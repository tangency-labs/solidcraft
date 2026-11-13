# ClassifyLoops' loop bisector builds UV trim curves upstream, on a copy in the port (2026-10-24)

`SmFace::ClassifyLoops` (`query.face_p4`) now calls the ported
`SmCurveClassification::GetLoopCrossings` for a target loop that crosses no seam. Before,
an `UNPORTED(intersect.curve_class_p4)` stub was left in place after that unit was
ported, and about 410 heal-path cases deferred at it. Those cases now compare, and 40 of
them disagree. They are listed in `crates/difftest/known_mismatches.toml`:
`heal.heal_data_p1` 26, `topology.brep_p1` 8, `topology.brep_p10` 6.

- **Root cause.** Upstream's bisector edits the real B-rep. The chain is
  `sms_FindAndClassifyLoopBisector` → `SmFace::CurveOnClassify` →
  `SmCurveClassification::IntersectTSBound`, which calls
  `pVertexuse->ComputeUVPoint(sUV)` (SmCurveClass.cpp:15964) with the default
  `bOKToMakeUVTrimCurve=TRUE`. That builds missing UV trim curves, and
  `SmEdgeuse::SetUVTrimCurve` then resets the vertex/face gap caches of the edge use's
  vertex use and of its mate's (`InitVertexFaceGap`, SmEdgeuse.cpp:4537). The port runs
  the bisector on a private copy of the B-rep (`query/face/p3.rs`, `b.clone()`, and the
  read-only boundary views behind it). `classify_loops`, `FaceProps::set_props_stage3`
  and `HealData::cache_face_props_stage3` all take `&Brep`, so those edits are lost.
- **Values.** `heal.heal_data_p1` `file_plane_square` (plane_square.smb has no UV trim
  curves): `values.out[227]` is the face-gap length of the first vertex props' worst
  vertex use, cpp `-1.23456e-61` (`SM_UNDEF_DOUBLE`, cache reset), rust `0`.
  `topology.brep_p1` `plane_square_heal`: `values.out[317]` cpp 1 rust 0, `out[337]`
  cpp 0 rust 1 (healed B-rep flags). `topology.brep_p10` `gen0115`: `values.dbls[648]`
  cpp `2.917160020417993e-3` rust `2.9171595358599514e-3` (later healing works from
  different UV trim curves). In the other brep_p10 cases the healed B-rep's arrays have
  different lengths. Each case's first mismatch is in its known-mismatch entry.
- **Checked.** Running `SmEdge::IsSeam` or `ComputeUVSector` on the real B-rep before
  stage 3 does not remove the disagreement (port instrumented, not committed). The
  `IntersectTSBound` chain comes from reading upstream, not from a C++ trace.
- **Fix (follow-up).** Thread `&mut Brep` from `SetProps_Stage3` through `ClassifyLoops`
  to the bisector's `CurveOnClassify`, so the UV trims and gap resets reach the B-rep, as
  the `LazyBrep` tight box in `query::face::p3` already does. `LoopObjectCurves` borrows
  the B-rep for the classification, so it has to change as well.
