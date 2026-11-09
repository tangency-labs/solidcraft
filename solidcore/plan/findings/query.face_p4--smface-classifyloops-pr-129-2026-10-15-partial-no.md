# query.face_p4 (`SmFace::ClassifyLoops`, PR #129, 2026-10-15; partial, no disagreements)

- **What is compared.** `ClassifyLoops` classifies a target loop either from the seam
  touch lists the face props already hold (loops crossing a periodic seam: the no-area
  loops of faces without seam edge, and seam-straddling area loops) or from a new
  bisecting classification curve (`sms_FindAndClassifyLoopBisector`, unit
  `query.face_p3`, then `GetLoopCrossings`, unit `intersect.curve_class_p4`). The first
  path is fully ported and compared; any target loop needing the second defers the
  case. The face props are built in the shim exactly as `SetProps_Stage3` builds them
  before its call (copied prefix) and exported, so both sides classify from the same
  props; fresh props (computed inside by `SetProps_Stage2`) are compared only on open
  surfaces (on a closed surface `ClassifyLoops` then reads the empty `m_sLoopProps`).
- **`SmTArray::SetSize` zero-fills.** New `SmClassifyLoopIO` records are `smos_Calloc` /
  `memset` bytes, not the constructor's values: an unclassified record has
  `m_tLoopuseType` 0 and `m_bArea_Loop` `FALSE` (not 16999 / `UNSURE`). The port's
  `ZEROED_CLASSIFY_LOOP_IO` reproduces it; `LoopuseType::Zero` was added for it.
- **Vertex loops fall through.** A target vertex loop is recorded and marked, then the
  same pass still builds a classification curve for it (no `continue` upstream).
- **Each pass deletes the previous loop tree** (`if(rpLooptreeRoot) delete`): only the
  last pass's tree is returned. `SmTArray::FindElement` misses leave the no-area index
  at 0. Ported as is.
- **heal.face_props runner.** Wiring `ClassifyLoops` into `SetProps_Stage3` showed that
  the Rust B-rep of the `heal.face_props` runner lacked the shim's single vertex loop
  (the earlier `query.face_p4` deferral hid it: 11 stage-3 cases then failed with
  `SM_ERR_NOTYET_HEAL_FACE` vs `SM_SUCCESS`); the runner now builds that loop and the
  cases defer on `query.face_p3`.
- **FFI guard beyond UB.** A seam touch connected to an object that is not a loop is
  rejected although upstream only compares the pointer: the port's loop tree holds loop
  ids. `SmCurveClassification` touch lists only ever connect to loops.
