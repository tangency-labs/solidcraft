# topology.face_p2: `sm_FixUVTrimCurves` no longer stretches the UV trims (issue #570, 2026-10-24)

- **Change.** `fix_uv_trim_curves` follows tangency-labs/smlib@5af7070: the vertex's UV
  point is no longer dropped onto the two trims, which are no longer `Trim`med there and
  `EditParameterization`ed back over their intervals. Only their end control points move,
  by the UV gap. Upstream's stretch slides every interior parameter of a trim off its
  edge's (`plan/upstream-issues.md` WR-255). Documented deviation in the module header.
- **Difftest.** Unchanged: no case reaches the removed branch. The `FixUv` cases of
  `topology.face_p2` call the function on the fixture B-reps' own trims, whose ends lie on
  their vertices' UV points, with a nominal `uv_dist_between`. The drop point is then the
  trim's end, so upstream's `Trim` is skipped (`SM_ARE_SAME(dTCurr, dTCurrNew)`), and the
  callers (`CreateUVTrimCurves` in `topology.face_p3`, `CheckAndFixUVTrimCurves` in
  `topology.brep_p2` / `p7`) see the same. topology.face_p2 2692/2596/0/96, face_p3
  1781/1604/0/177, brep_p2 1709/1673/0/36, brep_p7 348/289/0/59 (cases/passed/failed/
  deferred), as on main. A case with a corner gap along the trim would show the
  deviation; the runner has no op that moves a trim's end, so the only coverage is the
  unit test `fix_uv_trim_curves_keeps_interior_parameters`, which fails on the upstream
  code path (interior knots 3.3333 -> 3.3324 for a 1e-3 overshoot) and passes here.
