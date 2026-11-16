# `SmFace::HasSeamProblem` callers wired (topology.face_p4; wire/seam-problem, 2026-10-22)

`SmFaceProps::SetProps_Stage2` (`heal::face_props`) and the seam checks of
`SmFace::MoveSeam` / `SplitAtSeam` (`topology::face::p1`) now call
`topology::face::p4::has_seam_problem` through `has_seam_problem_in_brep`, whose
`BrepSeamObjects` names the face's own B-rep objects with the caller's `LoopObject`
handles (the handle type must name topology, edge / trim curves, the face surface and
curve temporaries; `HealLoopHandle` gained `Temp`). No case changed outcome: every case
that deferred at `SmFace::HasSeamProblem` (heal.face_props 210, intersect.curve_class_p4
436, heal.heal_data_p1 49, topology.brep_p10 14, topology.brep_p7 8) now defers one call
later, at the seam curve: `Surface::create_seam_curve` builds an `SmCrvOnSurf`, which
holds B-spline surfaces only (`curve.crv_on_surf`; the analytic periodic surfaces of the
fixtures, cylinder / sphere / torus / revolution), or at `SmCurveClassification::ReSet`
(`curve/crv_on_surf.rs`, `intersect.curve_class_p2`, heal_data_p1). Those are the real
blockers of the seam cases (generic-surface `SmCrvOnSurf`: open PR #307). Left unwired:
`topology::trimming_tools::face_has_seam_problem` (its `SeamObject` handles name loops and
vertices only, and its `SplitAtSeam` geometry resolves no curve; no difftest case
reaches it).
- **Follow-up merge through `339740fe`.** #446, #477, #484 and #486 landed during the
  run. They touch six kernel files (`blend::fillet_executive::p3`, `heal::face_props`,
  `topology::brep::p1`, `topology::face::p1`/`p4`) with no new self-call, call cycle
  or nested-geometry install; fmt, clippy, 1832 solidcore tests, 39 difftest tests
  and the wasm check pass.
