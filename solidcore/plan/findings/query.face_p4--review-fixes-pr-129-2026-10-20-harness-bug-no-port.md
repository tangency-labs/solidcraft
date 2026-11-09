# query.face_p4: review fixes (PR #129, 2026-10-20; harness bug, no port disagreement)

- **Out-param start values differed between the sides.** The shim's `run_op` started
  `ClassifyLoops`' out-params from `(SmHealNotYetFaceType)7` and `(SmCrvDirOnFaceType)99`,
  while the runner's `initial_out` starts the Rust side from `NoLoopTreeRoot` (3) and
  `Lamina` (5). The `SM_ERR` early return (empty target loops on a face with no loops,
  before any out-param is set) therefore failed the gate although both sides returned
  status 1001: a sphere face made with `MakeFaceWithCurves` from spans at v=1e-9 and
  v=1-1e-9 ends with no loops, and compared classifiable cpp=7 vs rust=3, start/stop
  [99,99] vs [5,5]. No earlier case reached that path. The shim now starts from
  `SM_HNF_NO_LOOP_TREE_ROOT` and `SM_CD_LAMINA` (= 5 in `SmTypes.h`), as the runner does.
  `fixtures/query.face_p4/adversarial.json` adds the reviewer's 41 adversarial cases,
  4 of them regressions for the no-loops path: 500 cases, 395 pass, 0 failed,
  105 deferred (query.face_p3), --perturb 395/395. After merging main (query.face_p3
  wired): 500 cases, 395 pass, 0 failed, 105 deferred (topology.face_p2).
- `classify_loops` now borrows the face props' done touch list (upstream holds a
  pointer) instead of cloning it per target loop; performance only.
- Not changed: `staged_guard` rejects a case whenever any touch in either seam done list
  has `m_lLoopIndx >= n_target`, though upstream reads `rClassifyLoopIOs[m_lLoopIndx]`
  only when it walks the selected list. It over-rejects about 3% of generated cases;
  narrowing it means replaying upstream's list selection and early returns in the shim.
